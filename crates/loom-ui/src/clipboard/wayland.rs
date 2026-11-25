//! The clipboard's read on Wayland, through `wl-clipboard-rs`, with a
//! deadline (ui-layer.md, section 8, rule 2). `arboard` reads the same
//! transfer with no limit, so a clipboard owner that stops holds the read
//! for good; this one ends at [`WAYLAND_READ_LIMIT`](super::WAYLAND_READ_LIMIT)
//! and closes the pipe. Writes still go through `arboard`.

use super::MAX_TEXT;
use super::pipe::read_within;
use crate::effect::ClipboardText;
use std::io::Read;
use std::os::fd::AsFd;
use std::time::Duration;
use wl_clipboard_rs::paste::{ClipboardType, Error, MimeType, Seat, get_contents};
use wl_clipboard_rs::utils::is_primary_selection_supported;

/// Whether `arboard`, opened in this session, chose the Wayland
/// data-control protocol, so our read must too: the read and the write
/// then always use the same clipboard. It makes the same check as
/// `arboard` 3.7 (`src/platform/linux/mod.rs`, `Clipboard::new`): a
/// `WAYLAND_DISPLAY`, and a compositor that answers the data-control
/// check. Otherwise `arboard` uses X11, and so do we. *Check this* when
/// `arboard` is upgraded: if its choice changes, this must change with it.
pub(super) fn arboard_uses_wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some() && is_primary_selection_supported().is_ok()
}

/// The clipboard's text, read within `limit` and at most
/// [`MAX_TEXT`](super::MAX_TEXT) bytes. An empty clipboard, or one with no
/// text, is empty, as `arboard` answers it; anything else that goes wrong,
/// a text past the cap too, is refused.
pub(super) fn read_text(limit: Duration) -> ClipboardText {
    answer(
        get_contents(ClipboardType::Regular, Seat::Unspecified, MimeType::Text),
        limit,
    )
}

/// What the compositor's answer gives: the pipe's text, read within
/// `limit`, or what its error means. *Why apart:* a test gives it a pipe of
/// its own and each error, with no compositor.
fn answer<P: Read + AsFd>(contents: Result<(P, String), Error>, limit: Duration) -> ClipboardText {
    match contents {
        // The pipe is dropped on return, which closes our end.
        Ok((mut pipe, _)) => super::text_of(read_within(&mut pipe, limit, MAX_TEXT).ok()),
        Err(Error::ClipboardEmpty | Error::NoMimeType) => ClipboardText::Empty,
        Err(_) => ClipboardText::Refused,
    }
}

#[cfg(test)]
mod tests;
