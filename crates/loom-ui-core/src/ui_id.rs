//! UI IDs: the stable identity of an element, and the table that gives each
//! one a number.
//!
//! An element is rebuilt many times while a window is open: a row is drawn
//! again each time its panel changes. A [`UiId`] says which element it is
//! across those rebuilds, so the state that belongs to it (keyboard focus,
//! a scroll offset, its accessibility node) stays with it.
//!
//! # Rules
//!
//! - **A key comes from what the element shows, not from where it sits.** A
//!   row for the file `notes.txt` is keyed by that name, not by "row 3".
//!   Removing the row above it then moves nothing onto it. A position is a
//!   fair key only in a list that never changes length.
//! - **An ID is a path.** The first key names the part of the window (a
//!   toolbar, a side panel). The keys after it narrow down to one element
//!   inside that part.
//! - **A number from the table is never used twice.** See [`IdTable`].
//!
//! # As text
//!
//! An ID can be written as text and read back, so a person, a script, and a
//! test can name the same element: the keys joined by `/`, as in
//! `toolbar/save` or `files/12/rename`.
//!
//! - A key made only of digits is a number: `12` is `IdKey::Number(12)`.
//! - Any other key is a name.
//! - Inside a name, `/` is written `%2F` and `%` is written `%25`, so a name
//!   that holds a path stays one key: `files/docs%2Fnotes.txt`.
//! - A name made only of digits has its first digit escaped, so it does not
//!   read back as a number: the name `"12"` is written `%312`.
//!
//! Reading takes any `%` followed by two hex digits as that byte. Writing an
//! ID and reading the text gives the same ID, for every ID.

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

#[cfg(test)]
mod tests;

/// One step of a [`UiId`]: a number or a short name.
///
/// Use a number for something the app already numbers, such as a record's
/// ID, and a name for something it names, such as `"save"` or a file name.
/// `Number(1)` and `Name("1")` are different keys.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IdKey {
    /// A number the app gave the thing shown.
    Number(u64),
    /// A name the app gave the thing shown.
    Name(Box<str>),
}

impl From<u64> for IdKey {
    fn from(number: u64) -> Self {
        Self::Number(number)
    }
}

impl From<u32> for IdKey {
    fn from(number: u32) -> Self {
        Self::Number(u64::from(number))
    }
}

impl From<&str> for IdKey {
    fn from(name: &str) -> Self {
        Self::Name(name.into())
    }
}

impl From<String> for IdKey {
    fn from(name: String) -> Self {
        Self::Name(name.into_boxed_str())
    }
}

/// The stable identity of one element: the key of its part of the window,
/// then the keys that lead to it inside that part.
///
/// ```
/// use loom_ui_core::UiId;
///
/// let toolbar = UiId::panel("toolbar");
/// let save = toolbar.clone().with("save");
/// let row = UiId::panel("files").with(12u32).with("rename");
///
/// assert!(save.is_under(&toolbar));
/// assert!(!row.is_under(&toolbar));
/// assert_eq!(save, UiId::panel("toolbar").with("save"));
///
/// // As text, and back.
/// assert_eq!(row.to_string(), "files/12/rename");
/// assert_eq!("files/12/rename".parse::<UiId>(), Ok(row));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UiId {
    /// Never empty: the first key is the panel's.
    keys: Vec<IdKey>,
}

impl UiId {
    /// The ID of a part of the window, which is also the start of the ID of
    /// everything inside it.
    pub fn panel(key: impl Into<IdKey>) -> Self {
        Self { keys: vec![key.into()] }
    }

    /// This ID with one more key: an element inside the one this names.
    #[must_use]
    pub fn with(mut self, key: impl Into<IdKey>) -> Self {
        self.keys.push(key.into());
        self
    }

    /// The keys, from the panel's key inwards. Never empty.
    pub fn keys(&self) -> &[IdKey] {
        &self.keys
    }

    /// True if this names `outer` itself or an element inside it.
    pub fn is_under(&self, outer: &Self) -> bool {
        self.keys.starts_with(&outer.keys)
    }
}

impl fmt::Display for IdKey {
    /// The key as it is written in an ID's text form, with `/`, `%`, and a
    /// name of digits escaped.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(number) => write!(f, "{number}"),
            Self::Name(name) => {
                let digits = !name.is_empty() && name.bytes().all(|b| b.is_ascii_digit());
                for (at, c) in name.chars().enumerate() {
                    if c == '/' || c == '%' || (digits && at == 0) {
                        write!(f, "%{:02X}", u32::from(c))?;
                    } else {
                        write!(f, "{c}")?;
                    }
                }
                Ok(())
            }
        }
    }
}

impl fmt::Display for UiId {
    /// The ID as text: its keys joined by `/`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (at, key) in self.keys.iter().enumerate() {
            if at > 0 {
                f.write_str("/")?;
            }
            write!(f, "{key}")?;
        }
        Ok(())
    }
}

/// Why a text is not a UI ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseUiIdError {
    /// A `%` is not followed by two hex digits. Holds the key it is in.
    BadEscape(String),
    /// The escapes of a key do not spell valid UTF-8. Holds the key.
    NotText(String),
    /// A key of digits is too large for a number. Holds the key.
    NumberTooLarge(String),
}

impl fmt::Display for ParseUiIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadEscape(key) => write!(f, "`{key}`: a `%` must be followed by two hex digits"),
            Self::NotText(key) => write!(f, "`{key}`: its escapes do not spell text"),
            Self::NumberTooLarge(key) => write!(f, "`{key}` is too large for a number"),
        }
    }
}

impl std::error::Error for ParseUiIdError {}

impl FromStr for IdKey {
    type Err = ParseUiIdError;

    /// Reads one key of an ID's text form. The text must hold no `/`.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) {
            return text
                .parse()
                .map(Self::Number)
                .map_err(|_| ParseUiIdError::NumberTooLarge(text.to_string()));
        }
        let bad = || ParseUiIdError::BadEscape(text.to_string());
        let mut bytes = Vec::with_capacity(text.len());
        let mut rest = text.bytes();
        while let Some(byte) = rest.next() {
            if byte == b'%' {
                let high = rest.next().and_then(|b| char::from(b).to_digit(16)).ok_or_else(bad)?;
                let low = rest.next().and_then(|b| char::from(b).to_digit(16)).ok_or_else(bad)?;
                bytes.push((high * 16 + low) as u8);
            } else {
                bytes.push(byte);
            }
        }
        String::from_utf8(bytes)
            .map(Self::from)
            .map_err(|_| ParseUiIdError::NotText(text.to_string()))
    }
}

impl FromStr for UiId {
    type Err = ParseUiIdError;

    /// Reads an ID from its text form: keys joined by `/`.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let keys = text.split('/').map(str::parse).collect::<Result<Vec<IdKey>, _>>()?;
        Ok(Self { keys })
    }
}

/// Gives each [`UiId`] a number that stays the same while its element
/// exists, and is never given to another element.
///
/// A screen reader keeps its place by such a number. If a number moved to
/// another element, the screen reader would read the wrong one. So:
///
/// - An ID asked for on every build keeps its number.
/// - Numbers come from a counter that only goes up.
/// - An ID that is gone loses its entry, so the table stays as large as the
///   window, not as long as the session. If the same ID comes back later,
///   it gets a new number.
///
/// "Gone" is decided by use. Ask for the number of every element with
/// [`number`](Self::number) while building, then call
/// [`retain_seen`](Self::retain_seen): every ID not asked for since the
/// last call is dropped.
///
/// ```
/// use loom_ui_core::{IdTable, UiId};
///
/// let mut table = IdTable::new();
/// let save = UiId::panel("toolbar").with("save");
///
/// let first = table.number(&save);
/// table.retain_seen();
/// assert_eq!(table.number(&save), first, "still shown, same number");
/// table.retain_seen();
///
/// // A build that does not show it, then one that shows it again.
/// table.retain_seen();
/// assert_ne!(table.number(&save), first, "it came back as a new element");
/// ```
#[derive(Debug, Clone)]
pub struct IdTable {
    /// The next number to give out. Only ever increases.
    next: u64,
    by_id: HashMap<UiId, Entry>,
    by_number: HashMap<u64, UiId>,
}

/// One ID's number, and whether it was asked for since the last sweep.
#[derive(Debug, Clone, Copy)]
struct Entry {
    number: u64,
    seen: bool,
}

impl Default for IdTable {
    fn default() -> Self {
        Self::new()
    }
}

impl IdTable {
    /// An empty table. The first number it gives is 1.
    pub fn new() -> Self {
        Self {
            next: 1,
            by_id: HashMap::new(),
            by_number: HashMap::new(),
        }
    }

    /// The number of `id`: the one it has, or a new one. Marks the ID as
    /// seen, so the next [`retain_seen`](Self::retain_seen) keeps it.
    pub fn number(&mut self, id: &UiId) -> u64 {
        if let Some(entry) = self.by_id.get_mut(id) {
            entry.seen = true;
            return entry.number;
        }
        let number = self.next;
        self.next += 1;
        self.by_id.insert(id.clone(), Entry { number, seen: true });
        self.by_number.insert(number, id.clone());
        number
    }

    /// The ID that has `number`, if its element still exists.
    pub fn id(&self, number: u64) -> Option<&UiId> {
        self.by_number.get(&number)
    }

    /// Drops every ID not asked for since the last call, and starts
    /// watching again. Call it once after each build.
    pub fn retain_seen(&mut self) {
        let by_number = &mut self.by_number;
        self.by_id.retain(|_, entry| {
            let keep = std::mem::take(&mut entry.seen);
            if !keep {
                by_number.remove(&entry.number);
            }
            keep
        });
    }

    /// Drops `outer` and every ID inside it, seen or not. For a part of the
    /// window whose content was replaced whole: its elements are new ones,
    /// even where their keys repeat.
    pub fn forget(&mut self, outer: &UiId) {
        let by_number = &mut self.by_number;
        self.by_id.retain(|id, entry| {
            let keep = !id.is_under(outer);
            if !keep {
                by_number.remove(&entry.number);
            }
            keep
        });
    }

    /// How many IDs have a number.
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// True while no ID has a number.
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}
