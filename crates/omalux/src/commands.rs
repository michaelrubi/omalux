//! Every user action, with its label and default shortcut (Lightroom
//! Classic's, where it has one). The keyboard, and headless scripts, both
//! go through this list, so they can't disagree. As in Omacull.

use egui::{Key, KeyboardShortcut, Modifiers};
use omalux_engine::shoot::{PICK, Rating};
use omalux_engine::sidecar::REJECT;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    Quit,
    Open,
    Close,
    Export,
    ExportAndEdit,
    Previous,
    Next,
    First,
    Last,
    /// The picks, or everything.
    Filter,
    Undo,
    Redo,
    /// The frame back to a fresh raw's settings.
    Reset,
    SliderPrevious,
    SliderNext,
    Increase,
    Decrease,
    IncreaseMore,
    DecreaseMore,
    /// The White Balance Selector: click something neutral.
    WhiteBalance,
    BeforeAfter,
    Reject,
    Unmark,
    Pick,
    Star1,
    Star2,
    Star3,
    Star4,
    Star5,
    /// Highlight and shadow clipping warnings.
    Clipping,
}

const CMD: Modifiers = Modifiers::COMMAND;
const CMD_SHIFT: Modifiers = Modifiers { shift: true, ..Modifiers::COMMAND };

impl Command {
    pub const ALL: &[Command] = &[
        Command::Quit,
        Command::Open,
        Command::Close,
        Command::Export,
        Command::ExportAndEdit,
        Command::Previous,
        Command::Next,
        Command::First,
        Command::Last,
        Command::Filter,
        Command::Undo,
        Command::Redo,
        Command::Reset,
        Command::SliderPrevious,
        Command::SliderNext,
        Command::Increase,
        Command::Decrease,
        Command::IncreaseMore,
        Command::DecreaseMore,
        Command::WhiteBalance,
        Command::BeforeAfter,
        Command::Reject,
        Command::Unmark,
        Command::Pick,
        Command::Star1,
        Command::Star2,
        Command::Star3,
        Command::Star4,
        Command::Star5,
        Command::Clipping,
    ];

    /// Look a command up by its name in code, e.g. "Open".
    pub fn from_name(name: &str) -> Option<Command> {
        Self::ALL.iter().copied().find(|c| format!("{c:?}") == name)
    }

    /// Every command with a shortcut, most modifiers first. egui matches
    /// Ctrl+Z even when Shift is also held, so Ctrl+Shift+Z has to be
    /// checked before it.
    pub const KEYBOARD_ORDER: &[Command] = &[
        Command::Redo,
        Command::Reset,
        Command::Undo,
        Command::Quit,
        Command::Open,
        Command::Close,
        Command::Export,
        Command::ExportAndEdit,
        Command::Filter,
        Command::Previous,
        Command::Next,
        Command::First,
        Command::Last,
        Command::IncreaseMore,
        Command::DecreaseMore,
        Command::Increase,
        Command::Decrease,
        Command::SliderPrevious,
        Command::SliderNext,
        Command::WhiteBalance,
        Command::BeforeAfter,
        Command::Reject,
        Command::Unmark,
        Command::Pick,
        Command::Star1,
        Command::Star2,
        Command::Star3,
        Command::Star4,
        Command::Star5,
        Command::Clipping,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Command::Quit => "Quit",
            Command::Open => "Open Folder…",
            Command::Close => "Close Folder",
            Command::Export => "Export",
            Command::ExportAndEdit => "Export and Edit in Omapix",
            Command::Previous => "Previous Frame",
            Command::Next => "Next Frame",
            Command::First => "First Frame",
            Command::Last => "Last Frame",
            Command::Filter => "Show the Picks or Everything",
            Command::Undo => "Undo",
            Command::Redo => "Redo",
            Command::Reset => "Reset the Frame",
            Command::SliderPrevious => "Select the Previous Slider",
            Command::SliderNext => "Select the Next Slider",
            Command::Increase => "Increase the Selected Slider",
            Command::Decrease => "Decrease the Selected Slider",
            Command::IncreaseMore => "Increase the Selected Slider More",
            Command::DecreaseMore => "Decrease the Selected Slider More",
            Command::WhiteBalance => "White Balance Selector",
            Command::BeforeAfter => "Before/After",
            Command::Reject => "Reject",
            Command::Unmark => "Unmark",
            Command::Pick => "Pick",
            Command::Star1 => "1 Star",
            Command::Star2 => "2 Stars",
            Command::Star3 => "3 Stars",
            Command::Star4 => "4 Stars",
            Command::Star5 => "5 Stars",
            Command::Clipping => "Clipping Warnings",
        }
    }

    pub fn default_shortcut(self) -> Option<KeyboardShortcut> {
        let s = |m, k| Some(KeyboardShortcut::new(m, k));
        match self {
            Command::Quit => s(CMD, Key::Q),
            Command::Open => s(CMD, Key::O),
            Command::Close => s(CMD, Key::W),
            Command::Export => s(CMD_SHIFT, Key::E),
            Command::ExportAndEdit => s(CMD, Key::E),
            Command::Previous => s(Modifiers::NONE, Key::ArrowLeft),
            Command::Next => s(Modifiers::NONE, Key::ArrowRight),
            Command::First => s(Modifiers::NONE, Key::Home),
            Command::Last => s(Modifiers::NONE, Key::End),
            // Lightroom's Enable Filters.
            Command::Filter => s(CMD, Key::L),
            Command::Undo => s(CMD, Key::Z),
            Command::Redo => s(CMD_SHIFT, Key::Z),
            Command::Reset => s(CMD_SHIFT, Key::R),
            Command::SliderPrevious => s(Modifiers::NONE, Key::Comma),
            Command::SliderNext => s(Modifiers::NONE, Key::Period),
            Command::Increase => s(Modifiers::NONE, Key::Equals),
            Command::Decrease => s(Modifiers::NONE, Key::Minus),
            // Shift with = is +.
            Command::IncreaseMore => s(Modifiers::SHIFT, Key::Plus),
            Command::DecreaseMore => s(Modifiers::SHIFT, Key::Minus),
            Command::WhiteBalance => s(Modifiers::NONE, Key::W),
            Command::BeforeAfter => s(Modifiers::NONE, Key::Backslash),
            Command::Reject => s(Modifiers::NONE, Key::X),
            Command::Unmark => s(Modifiers::NONE, Key::U),
            Command::Pick => s(Modifiers::NONE, Key::P),
            Command::Star1 => s(Modifiers::NONE, Key::Num1),
            Command::Star2 => s(Modifiers::NONE, Key::Num2),
            Command::Star3 => s(Modifiers::NONE, Key::Num3),
            Command::Star4 => s(Modifiers::NONE, Key::Num4),
            Command::Star5 => s(Modifiers::NONE, Key::Num5),
            // Lightroom's Show Clipping.
            Command::Clipping => s(Modifiers::NONE, Key::J),
        }
    }

    /// The mark a marking command makes.
    pub fn rating(self) -> Option<Rating> {
        Some(match self {
            Command::Reject => REJECT,
            Command::Unmark => 0,
            Command::Pick => PICK,
            Command::Star1 => 1,
            Command::Star2 => 2,
            Command::Star3 => 3,
            Command::Star4 => 4,
            Command::Star5 => 5,
            _ => return None,
        })
    }

    /// Commands whose shortcut was pressed this frame, consuming the keys.
    pub fn pressed(ctx: &egui::Context) -> Vec<Command> {
        Self::pressed_with(ctx, crate::hotkeys::current())
    }

    /// [`Self::pressed`] with these shortcuts.
    pub fn pressed_with(ctx: &egui::Context, hotkeys: &crate::hotkeys::Hotkeys) -> Vec<Command> {
        ctx.input_mut(|i| {
            let mut pressed: Vec<Command> = hotkeys
                .keyboard_order()
                .iter()
                .copied()
                .filter(|&c| hotkeys.command(c).is_some_and(|s| i.consume_shortcut(&s)))
                .collect();
            // 0 is no stars, as in Bridge and Lightroom, unless it was
            // given to another command (which has consumed it by now).
            if hotkeys.command(Command::Unmark).is_some()
                && i.consume_key(Modifiers::NONE, Key::Num0)
                && !pressed.contains(&Command::Unmark)
            {
                pressed.push(Command::Unmark);
            }
            pressed
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
        assert_eq!(press(&ctx, CMD, Key::W), [Command::Close]);
        assert_eq!(press(&ctx, CMD_SHIFT, Key::E), [Command::Export]);
        assert_eq!(press(&ctx, CMD, Key::E), [Command::ExportAndEdit]);
        assert_eq!(press(&ctx, Modifiers::NONE, Key::O), []);
        assert_eq!(press(&ctx, Modifiers::NONE, Key::ArrowRight), [Command::Next]);
        assert_eq!(press(&ctx, Modifiers::NONE, Key::Home), [Command::First]);
        assert_eq!(press(&ctx, CMD, Key::L), [Command::Filter]);
        assert_eq!(press(&ctx, CMD, Key::Z), [Command::Undo]);
        assert_eq!(press(&ctx, CMD_SHIFT, Key::Z), [Command::Redo], "not mistaken for undo");
        assert_eq!(press(&ctx, CMD_SHIFT, Key::R), [Command::Reset]);
        assert_eq!(press(&ctx, Modifiers::NONE, Key::Period), [Command::SliderNext]);
        assert_eq!(press(&ctx, Modifiers::NONE, Key::Minus), [Command::Decrease]);
        assert_eq!(press(&ctx, Modifiers::SHIFT, Key::Minus), [Command::DecreaseMore]);
        assert_eq!(press(&ctx, Modifiers::SHIFT, Key::Plus), [Command::IncreaseMore]);
        assert_eq!(press(&ctx, Modifiers::NONE, Key::W), [Command::WhiteBalance]);
        assert_eq!(press(&ctx, Modifiers::NONE, Key::Backslash), [Command::BeforeAfter]);
    }

    #[test]
    fn marks_and_steps_are_single_keys() {
        let ctx = egui::Context::default();
        let none = Modifiers::NONE;
        assert_eq!(press(&ctx, none, Key::X), [Command::Reject]);
        assert_eq!(press(&ctx, none, Key::U), [Command::Unmark]);
        assert_eq!(press(&ctx, none, Key::Num0), [Command::Unmark]);
        assert_eq!(press(&ctx, none, Key::P), [Command::Pick]);
        assert_eq!(press(&ctx, none, Key::Num1), [Command::Star1]);
        assert_eq!(press(&ctx, none, Key::Num2), [Command::Star2]);
        assert_eq!(press(&ctx, none, Key::Num3), [Command::Star3]);
        assert_eq!(press(&ctx, none, Key::Num4), [Command::Star4]);
        assert_eq!(press(&ctx, none, Key::Num5), [Command::Star5]);
        assert_eq!(press(&ctx, none, Key::ArrowRight), [Command::Next]);
        assert_eq!(press(&ctx, none, Key::Home), [Command::First]);
        assert_eq!(press(&ctx, Modifiers::NONE, Key::J), [Command::Clipping]);
    }
}
