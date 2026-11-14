//! The hierarchy's links, kept in two orders, so the follows through ports
//! and empty thru slots find a link by range, not by scanning every link of
//! the patch: a crafted file of long chains must not freeze the editor
//! (sub-patches.md, Empty slots).

use super::{End, Link};
use crate::ModulePath;
use std::collections::BTreeSet;

/// Every link, ordered by level and input (as [`Link`] orders), and again
/// by level and output.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct LinkSet {
    /// By level, then input, then output.
    by_to: BTreeSet<Link>,
    /// By level, then output, then input.
    by_from: BTreeSet<(ModulePath, End, End)>,
}

/// The smallest end, the start of a range over one level.
fn first_end() -> End {
    (0, String::new())
}

impl LinkSet {
    /// Adds `link`; false when it was there already.
    pub(super) fn insert(&mut self, link: Link) -> bool {
        self.by_from
            .insert((link.level.clone(), link.from.clone(), link.to.clone()));
        self.by_to.insert(link)
    }

    /// Removes `link`; false when it was not there.
    pub(super) fn remove(&mut self, link: &Link) -> bool {
        self.by_from
            .remove(&(link.level.clone(), link.from.clone(), link.to.clone()));
        self.by_to.remove(link)
    }

    /// Keeps only the links `keep` accepts.
    pub(super) fn retain(&mut self, mut keep: impl FnMut(&Link) -> bool) {
        let gone: Vec<Link> = self.by_to.iter().filter(|l| !keep(l)).cloned().collect();
        for link in &gone {
            self.remove(link);
        }
    }

    /// Every link, by level and input.
    pub(super) fn iter(&self) -> impl Iterator<Item = &Link> {
        self.by_to.iter()
    }

    /// How many links there are.
    pub(super) fn len(&self) -> usize {
        self.by_to.len()
    }

    /// The links of `level`, by range.
    pub(super) fn at_level<'a>(&'a self, level: &ModulePath) -> impl Iterator<Item = &'a Link> + 'a {
        let start = Link {
            level: level.clone(),
            to: first_end(),
            from: first_end(),
        };
        let level = level.clone();
        self.by_to.range(start..).take_while(move |l| l.level == level)
    }

    /// The one link into `to` on `level`, by range.
    pub(super) fn linked_into(&self, level: &ModulePath, to: &End) -> Option<&Link> {
        let start = Link {
            level: level.clone(),
            to: to.clone(),
            from: first_end(),
        };
        self.by_to
            .range(start..)
            .next()
            .filter(|l| l.level == *level && l.to == *to)
    }

    /// The links out of `from` on `level`, by range, as the links they are.
    pub(super) fn linked_from<'a>(&'a self, level: &ModulePath, from: &End) -> impl Iterator<Item = Link> + 'a {
        let start = (level.clone(), from.clone(), first_end());
        let (level, from) = (level.clone(), from.clone());
        self.by_from
            .range(start..)
            .take_while(move |(l, f, _)| *l == level && *f == from)
            .map(|(level, from, to)| Link {
                level: level.clone(),
                to: to.clone(),
                from: from.clone(),
            })
    }
}

impl<'a> IntoIterator for &'a LinkSet {
    type Item = &'a Link;
    type IntoIter = std::collections::btree_set::Iter<'a, Link>;

    fn into_iter(self) -> Self::IntoIter {
        self.by_to.iter()
    }
}

impl IntoIterator for LinkSet {
    type Item = Link;
    type IntoIter = std::collections::btree_set::IntoIter<Link>;

    fn into_iter(self) -> Self::IntoIter {
        self.by_to.into_iter()
    }
}

impl FromIterator<Link> for LinkSet {
    fn from_iter<I: IntoIterator<Item = Link>>(iter: I) -> Self {
        let mut set = Self::default();
        for link in iter {
            set.insert(link);
        }
        set
    }
}

#[cfg(test)]
mod tests;
