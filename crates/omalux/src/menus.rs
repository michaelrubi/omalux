//! The right-click menus on the loupe and the filmstrip: the commands that
//! act on what was clicked, each with its key, so they can be found
//! without knowing it.
//!
//! Modelled on Omapix's layer and mask menus (`omapix/src/layers_panel.rs`).

use omalux_engine::shoot::Rating;

use crate::commands::Command;
use crate::hotkeys;
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

/// A menu's item: a command, or the Rating submenu of the stars.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Entry {
    Command(Command),
    Rating,
}

use Entry::{Command as C, Rating as Stars};

/// The loupe's menu, in groups between separators.
const LOUPE: &[&[Entry]] = &[
    &[C(Command::BeforeAfter), C(Command::Clipping), C(Command::Zoom)],
    &[C(Command::WhiteBalance), C(Command::Flat), C(Command::AutoTone), C(Command::AutoWhiteBalance), C(Command::Crop)],
    &[C(Command::Copy), C(Command::CopyChoose), C(Command::Paste), C(Command::PastePrevious)],
    &[C(Command::Pick), C(Command::Reject), C(Command::Unmark), Stars],
    &[C(Command::Reset)],
    &[C(Command::Export), C(Command::ExportAndEdit)],
    &[C(Command::Shortcuts)],
];

/// The filmstrip's.
const FILMSTRIP: &[&[Entry]] = &[
    &[C(Command::Pick), C(Command::Reject), C(Command::Unmark), Stars],
    &[C(Command::Copy), C(Command::Paste), C(Command::PasteStack)],
    &[C(Command::ToggleStack)],
    &[C(Command::SelectAll), C(Command::SelectNone)],
    &[C(Command::Reset), C(Command::Export)],
    &[C(Command::Shortcuts)],
];

const STARS: &[Command] = &[Command::Star1, Command::Star2, Command::Star3, Command::Star4, Command::Star5];

/// Whether `command` is in the menu at all: the stack's only in a stack.
fn visible(command: Command, context: &Context) -> bool {
    match command {
        Command::PasteStack | Command::ToggleStack => context.stack.is_some(),
        _ => true,
    }
}

/// Whether `command` can be done now.
fn enabled(command: Command, context: &Context) -> bool {
    match command {
        Command::Paste | Command::PasteStack => context.copied,
        Command::PastePrevious => context.previous,
        Command::SelectNone => context.selection,
        _ => true,
    }
}

/// Whether `command` is a toggle that's on, or the mark the frame has.
fn ticked(command: Command, context: &Context) -> bool {
    match command {
        Command::BeforeAfter => context.before,
        Command::Clipping => context.clipping,
        Command::Flat => context.flat,
        _ if STARS.contains(&command) => command.rating() == Some(context.rating),
        _ => false,
    }
}

/// What the item says: the command's label, but where a menu says what it
/// will do.
fn label(command: Command, context: &Context) -> &'static str {
    match command {
        Command::ToggleStack if context.stack == Some(true) => "Close Up the Stack",
        Command::ToggleStack => "Open Out the Stack",
        Command::Flat => "Flat Profile",
        _ => command.label(),
    }
}

/// One item: its label, a tick if it's on, its key at the right; greyed
/// out when it can't be done. The command, if it was clicked.
fn item(ui: &mut egui::Ui, theme: &Theme, context: &Context, command: Command) -> Option<Command> {
    if !visible(command, context) {
        return None;
    }
    let label = label(command, context);
    let mut button = egui::Button::new(if ticked(command, context) { format!("✓ {label}") } else { label.to_owned() });
    if let Some(shortcut) = hotkeys::current().command(command) {
        button = button.shortcut_text(egui::RichText::new(hotkeys::format_shortcut(&shortcut)).color(theme.muted));
    }
    ui.add_enabled(enabled(command, context), button).clicked().then(|| {
        ui.close();
        command
    })
}

/// A menu of `groups`, those with nothing to show left out.
fn menu(ui: &mut egui::Ui, theme: &Theme, context: &Context, groups: &[&[Entry]]) -> Option<Command> {
    let mut chosen = None;
    let shown = groups.iter().filter(|group| group.iter().any(|entry| !matches!(entry, C(command) if !visible(*command, context))));
    for (i, group) in shown.enumerate() {
        if i > 0 {
            ui.separator();
        }
        for entry in *group {
            match *entry {
                C(command) => chosen = item(ui, theme, context, command).or(chosen),
                Stars => {
                    ui.menu_button("Rating", |ui| {
                        for &star in STARS {
                            chosen = item(ui, theme, context, star).or(chosen);
                        }
                    });
                }
            }
        }
    }
    chosen
}

/// The loupe's menu, on `response`: the command chosen, if one was.
pub fn loupe(response: &egui::Response, theme: &Theme, context: &Context) -> Option<Command> {
    egui::Popup::context_menu(response).show(|ui| menu(ui, theme, context, LOUPE)).and_then(|shown| shown.inner)
}

/// The filmstrip's menu, on `response`: the command chosen, if one was.
/// The frame right-clicked is the current one by then.
pub fn filmstrip(response: &egui::Response, theme: &Theme, context: &Context) -> Option<Command> {
    egui::Popup::context_menu(response).show(|ui| menu(ui, theme, context, FILMSTRIP)).and_then(|shown| shown.inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_items_only_visible_when_in_stack() {
        let no_stack = Context { stack: None, ..Default::default() };
        assert!(!visible(Command::PasteStack, &no_stack));
        assert!(!visible(Command::ToggleStack, &no_stack));
        assert!(visible(Command::Copy, &no_stack));

        let closed_stack = Context { stack: Some(false), ..Default::default() };
        assert!(visible(Command::PasteStack, &closed_stack));
        assert!(visible(Command::ToggleStack, &closed_stack));

        let open_stack = Context { stack: Some(true), ..Default::default() };
        assert!(visible(Command::PasteStack, &open_stack));
        assert!(visible(Command::ToggleStack, &open_stack));
    }

    #[test]
    fn paste_commands_require_clipboard_or_previous() {
        let empty = Context::default();
        assert!(!enabled(Command::Paste, &empty));
        assert!(!enabled(Command::PasteStack, &empty));
        assert!(!enabled(Command::PastePrevious, &empty));

        let copied = Context { copied: true, ..Default::default() };
        assert!(enabled(Command::Paste, &copied));
        assert!(enabled(Command::PasteStack, &copied));
        assert!(!enabled(Command::PastePrevious, &copied));

        let previous = Context { previous: true, ..Default::default() };
        assert!(!enabled(Command::Paste, &previous));
        assert!(enabled(Command::PastePrevious, &previous));
    }

    #[test]
    fn select_none_requires_selection() {
        let no_selection = Context { selection: false, ..Default::default() };
        assert!(!enabled(Command::SelectNone, &no_selection));
        assert!(enabled(Command::SelectAll, &no_selection));

        let with_selection = Context { selection: true, ..Default::default() };
        assert!(enabled(Command::SelectNone, &with_selection));
        assert!(enabled(Command::SelectAll, &with_selection));
    }

    #[test]
    fn toggles_ticked_when_active() {
        let off = Context::default();
        assert!(!ticked(Command::BeforeAfter, &off));
        assert!(!ticked(Command::Clipping, &off));
        assert!(!ticked(Command::Flat, &off));
        assert!(!ticked(Command::Star1, &off));
        assert!(!ticked(Command::Star2, &off));
        assert!(!ticked(Command::Star3, &off));
        assert!(!ticked(Command::Star4, &off));
        assert!(!ticked(Command::Star5, &off));

        let on = Context { before: true, clipping: true, flat: true, rating: 3, ..Default::default() };
        assert!(ticked(Command::BeforeAfter, &on));
        assert!(ticked(Command::Clipping, &on));
        assert!(ticked(Command::Flat, &on));
        assert!(!ticked(Command::Star1, &on));
        assert!(!ticked(Command::Star2, &on));
        assert!(ticked(Command::Star3, &on));
        assert!(!ticked(Command::Star4, &on));
        assert!(!ticked(Command::Star5, &on));
    }

    #[test]
    fn dynamic_labels() {
        let closed = Context { stack: Some(false), ..Default::default() };
        assert_eq!(label(Command::ToggleStack, &closed), "Open Out the Stack");

        let open = Context { stack: Some(true), ..Default::default() };
        assert_eq!(label(Command::ToggleStack, &open), "Close Up the Stack");

        assert_eq!(label(Command::Flat, &Context::default()), "Flat Profile");
        assert_eq!(label(Command::BeforeAfter, &Context::default()), "Before/After");
    }
}
