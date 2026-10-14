//! The right-click menus on the loupe and the filmstrip: the commands that
//! act on what was clicked, each with its key, so they can be found
//! without knowing it.

use omalux_engine::shoot::Rating;

use crate::commands::Command;
use crate::theme::Theme;

/// What the menus need to know about the frame and the app, to tick what's
/// on and leave out what can't be done.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Context {
    /// The current frame's mark.
    pub rating: Rating,
    /// Settings have been copied: there's something to paste.
    pub copied: bool,
    /// There's a frame the cursor came from, to paste from.
    pub previous: bool,
    /// The current frame is in a stack, and whether it's open.
    pub stack: Option<bool>,
    /// Frames are selected besides the current one.
    pub selection: bool,
    pub before: bool,
    pub clipping: bool,
    /// The Flat profile is on.
    pub flat: bool,
}

/// The loupe's menu, on `response`: the command chosen, if one was.
pub fn loupe(response: &egui::Response, theme: &Theme, context: &Context) -> Option<Command> {
    let _ = (response, theme, context);
    None
}

/// The filmstrip's menu, on `response`: the command chosen, if one was.
/// The frame right-clicked is the current one by then.
pub fn filmstrip(response: &egui::Response, theme: &Theme, context: &Context) -> Option<Command> {
    let _ = (response, theme, context);
    None
}
