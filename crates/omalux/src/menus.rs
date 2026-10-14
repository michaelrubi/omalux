//! The right-click menus on the loupe and the filmstrip: the commands that
//! act on what was clicked, each with its key, so they can be found
//! without knowing it.
//!
//! Modelled on Omapix's layer and mask context menus in layers_panel.rs.

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

#[derive(Clone, Copy, Debug, PartialEq)]
enum Entry {
    Command(Command),
    RatingSubmenu,
}

const LOUPE_GROUPS: &[&[Entry]] = &[
    &[
        Entry::Command(Command::BeforeAfter),
        Entry::Command(Command::Clipping),
        Entry::Command(Command::Zoom),
    ],
    &[
        Entry::Command(Command::WhiteBalance),
        Entry::Command(Command::Flat),
        Entry::Command(Command::AutoTone),
        Entry::Command(Command::AutoWhiteBalance),
        Entry::Command(Command::Crop),
    ],
    &[
        Entry::Command(Command::Copy),
        Entry::Command(Command::CopyChoose),
        Entry::Command(Command::Paste),
        Entry::Command(Command::PastePrevious),
    ],
    &[
        Entry::Command(Command::Pick),
        Entry::Command(Command::Reject),
        Entry::Command(Command::Unmark),
        Entry::RatingSubmenu,
    ],
    &[
        Entry::Command(Command::Reset),
    ],
    &[
        Entry::Command(Command::Export),
        Entry::Command(Command::ExportAndEdit),
    ],
    &[
        Entry::Command(Command::Shortcuts),
    ],
];

const FILMSTRIP_GROUPS: &[&[Entry]] = &[
    &[
        Entry::Command(Command::Pick),
        Entry::Command(Command::Reject),
        Entry::Command(Command::Unmark),
        Entry::RatingSubmenu,
    ],
    &[
        Entry::Command(Command::Copy),
        Entry::Command(Command::Paste),
        Entry::Command(Command::PasteStack),
    ],
    &[
        Entry::Command(Command::ToggleStack),
    ],
    &[
        Entry::Command(Command::SelectAll),
        Entry::Command(Command::SelectNone),
    ],
    &[
        Entry::Command(Command::Reset),
        Entry::Command(Command::Export),
    ],
    &[
        Entry::Command(Command::Shortcuts),
    ],
];

const RATING_STARS: &[Command] = &[
    Command::Star1,
    Command::Star2,
    Command::Star3,
    Command::Star4,
    Command::Star5,
];

/// Whether this command is shown in the menu for this context.
pub fn visible(command: Command, context: &Context) -> bool {
    match command {
        Command::PasteStack | Command::ToggleStack => context.stack.is_some(),
        _ => true,
    }
}

/// Whether this command can be run right now.
pub fn enabled(command: Command, context: &Context) -> bool {
    match command {
        Command::Paste | Command::PasteStack => context.copied,
        Command::PastePrevious => context.previous,
        Command::SelectNone => context.selection,
        _ => true,
    }
}

/// Whether this toggle command is on.
pub fn ticked(command: Command, context: &Context) -> bool {
    match command {
        Command::BeforeAfter => context.before,
        Command::Clipping => context.clipping,
        Command::Flat => context.flat,
        Command::Star1 | Command::Star2 | Command::Star3 | Command::Star4 | Command::Star5 => {
            command.rating() == Some(context.rating)
        }
        _ => false,
    }
}

/// The label shown in the menu for this command.
pub fn label(command: Command, context: &Context) -> &'static str {
    match command {
        Command::ToggleStack => {
            if context.stack == Some(true) {
                "Close Up the Stack"
            } else {
                "Open Out the Stack"
            }
        }
        Command::Flat => "Flat Profile",
        _ => command.label(),
    }
}

/// Add one menu item button: label on the left, key on the right in muted
/// colour, greyed out when disabled, and a check mark when ticked.
fn menu_item(ui: &mut egui::Ui, theme: &Theme, context: &Context, command: Command) -> Option<Command> {
    if !visible(command, context) {
        return None;
    }
    let is_ticked = ticked(command, context);
    let is_enabled = enabled(command, context);
    let item_label = label(command, context);
    let text = if is_ticked {
        format!("✓ {item_label}")
    } else {
        item_label.to_owned()
    };
    let mut button = egui::Button::new(text);
    if let Some(shortcut) = crate::hotkeys::current().command(command) {
        let key_str = crate::hotkeys::format_shortcut(&shortcut);
        button = button.shortcut_text(egui::RichText::new(key_str).color(theme.muted));
    }
    if ui.add_enabled(is_enabled, button).clicked() {
        ui.close();
        Some(command)
    } else {
        None
    }
}

fn render_menu(
    ui: &mut egui::Ui,
    theme: &Theme,
    context: &Context,
    groups: &[&[Entry]],
) -> Option<Command> {
    let mut chosen = None;
    let mut rendered_any = false;

    for group in groups {
        let has_visible = group.iter().any(|entry| match entry {
            Entry::Command(cmd) => visible(*cmd, context),
            Entry::RatingSubmenu => true,
        });

        if !has_visible {
            continue;
        }

        if rendered_any {
            ui.separator();
        }

        for entry in *group {
            match entry {
                Entry::Command(cmd) => {
                    if let Some(c) = menu_item(ui, theme, context, *cmd) {
                        chosen = Some(c);
                    }
                }
                Entry::RatingSubmenu => {
                    ui.menu_button("Rating", |ui| {
                        for &star in RATING_STARS {
                            if let Some(c) = menu_item(ui, theme, context, star) {
                                chosen = Some(c);
                            }
                        }
                    });
                }
            }
        }

        rendered_any = true;
    }

    chosen
}

/// The loupe's menu, on `response`: the command chosen, if one was.
pub fn loupe(response: &egui::Response, theme: &Theme, context: &Context) -> Option<Command> {
    let mut chosen = None;
    egui::Popup::context_menu(response)
        .sense(egui::Sense::hover())
        .show(|ui| {
            chosen = render_menu(ui, theme, context, LOUPE_GROUPS);
        });
    chosen
}

/// The filmstrip's menu, on `response`: the command chosen, if one was.
/// The frame right-clicked is the current one by then.
pub fn filmstrip(response: &egui::Response, theme: &Theme, context: &Context) -> Option<Command> {
    let mut chosen = None;
    egui::Popup::context_menu(response)
        .sense(egui::Sense::hover())
        .show(|ui| {
            chosen = render_menu(ui, theme, context, FILMSTRIP_GROUPS);
        });
    chosen
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

        let on = Context {
            before: true,
            clipping: true,
            flat: true,
            rating: 3,
            ..Default::default()
        };
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
