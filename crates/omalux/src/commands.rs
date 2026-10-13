//! Every user action, with its label and default shortcut (Lightroom
//! Classic's, where it has one). The keyboard, and headless scripts, both
//! go through this list, so they can't disagree. As in Omacull.

use egui::{Key, KeyboardShortcut, Modifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    Quit,
    Open,
}

const CMD: Modifiers = Modifiers::COMMAND;

impl Command {
    pub const ALL: &[Command] = &[Command::Quit, Command::Open];

    /// Look a command up by its name in code, e.g. "Open".
    pub fn from_name(name: &str) -> Option<Command> {
        Self::ALL.iter().copied().find(|c| format!("{c:?}") == name)
    }

    /// Every command with a shortcut, most modifiers first. egui matches
    /// Ctrl+Z even when Shift is also held, so Ctrl+Shift+Z has to be
    /// checked before it.
    pub const KEYBOARD_ORDER: &[Command] = &[Command::Quit, Command::Open];

    pub fn label(self) -> &'static str {
        match self {
            Command::Quit => "Quit",
            Command::Open => "Open Folder…",
        }
    }

    pub fn default_shortcut(self) -> Option<KeyboardShortcut> {
        let s = |m, k| Some(KeyboardShortcut::new(m, k));
        match self {
            Command::Quit => s(CMD, Key::Q),
            Command::Open => s(CMD, Key::O),
        }
    }

    /// Commands whose shortcut was pressed this frame, consuming the keys.
    pub fn pressed(ctx: &egui::Context) -> Vec<Command> {
        Self::pressed_with(ctx, crate::hotkeys::current())
    }

    /// [`Self::pressed`] with these shortcuts.
    pub fn pressed_with(ctx: &egui::Context, hotkeys: &crate::hotkeys::Hotkeys) -> Vec<Command> {
        ctx.input_mut(|i| {
            hotkeys
                .keyboard_order()
                .iter()
                .copied()
                .filter(|&c| hotkeys.command(c).is_some_and(|s| i.consume_shortcut(&s)))
                .collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shortcut_is_in_keyboard_order() {
        for &c in Command::ALL {
            if c.default_shortcut().is_some() {
                assert!(Command::KEYBOARD_ORDER.contains(&c), "{c:?} missing from KEYBOARD_ORDER");
            }
        }
    }

    #[test]
    fn names_round_trip() {
        for &c in Command::ALL {
            assert_eq!(Command::from_name(&format!("{c:?}")), Some(c));
        }
    }

    /// Commands pressed in a frame with this key and these modifiers held.
    fn press(ctx: &egui::Context, modifiers: Modifiers, key: Key) -> Vec<Command> {
        let events = vec![
            egui::Event::ModifiersChanged(modifiers),
            egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers },
        ];
        let input = egui::RawInput { events, ..Default::default() };
        let mut pressed = Vec::new();
        let mut output = ctx.run_ui(input, |ui| pressed = Command::pressed(ui.ctx()));
        // There's no renderer to upload textures to.
        output.textures_delta.clear();
        pressed
    }

    #[test]
    fn lightrooms_keys() {
        let ctx = egui::Context::default();
        assert_eq!(press(&ctx, CMD, Key::Q), [Command::Quit]);
        assert_eq!(press(&ctx, CMD, Key::O), [Command::Open]);
        assert_eq!(press(&ctx, Modifiers::NONE, Key::O), []);
    }
}
