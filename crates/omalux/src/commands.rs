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
    /// 100%, or back to the whole frame: a tap toggles, a hold is a look.
    Zoom,
    SelectAll,
    SelectNone,
    /// Step back, taking the frames passed into the selection.
    SelectPrevious,
    SelectNext,
    /// Copy every setting of the current frame.
    Copy,
    /// Choose which groups of settings to copy.
    CopyChoose,
    /// Paste what was copied onto the selection, or the current frame.
    Paste,
    /// Paste the settings of the frame the cursor came from.
    PastePrevious,
    /// Paste what was copied onto the current frame's whole stack.
    PasteStack,
    /// Open out the current frame's stack in the filmstrip, or close it up.
    ToggleStack,
    /// Crop & Straighten: open the tool, or put it away.
    Crop,
    /// A quarter turn.
    RotateLeft,
    RotateRight,
    /// The whole frame again, unstraightened.
    ResetCrop,
    /// Whites and Blacks, so nothing clips that needn't.
    AutoTone,
    AutoWhiteBalance,
    AutoExposure,
    /// Straighten by the frame's own horizon or uprights.
    AutoLevel,
    /// The Flat profile, or Camera match again.
    Flat,
}

const CMD: Modifiers = Modifiers::COMMAND;
const CMD_SHIFT: Modifiers = Modifiers { shift: true, ..Modifiers::COMMAND };
const CMD_ALT: Modifiers = Modifiers { alt: true, ..Modifiers::COMMAND };

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
        Command::Zoom,
        Command::SelectAll,
        Command::SelectNone,
        Command::SelectPrevious,
        Command::SelectNext,
        Command::Copy,
        Command::CopyChoose,
        Command::Paste,
        Command::PastePrevious,
        Command::PasteStack,
        Command::ToggleStack,
        Command::Crop,
        Command::RotateLeft,
        Command::RotateRight,
        Command::ResetCrop,
        Command::AutoTone,
        Command::AutoWhiteBalance,
        Command::AutoExposure,
        Command::AutoLevel,
        Command::Flat,
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
        Command::Zoom,
        Command::SelectAll,
        Command::SelectNone,
        Command::SelectPrevious,
        Command::SelectNext,
        Command::Copy,
        Command::CopyChoose,
        Command::Paste,
        Command::PastePrevious,
        Command::PasteStack,
        Command::ToggleStack,
        Command::Crop,
        Command::RotateLeft,
        Command::RotateRight,
        Command::ResetCrop,
        Command::AutoWhiteBalance,
        Command::AutoTone,
        Command::Flat,
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
            Command::Zoom => "Zoom to 100%",
            Command::SelectAll => "Select All",
            Command::SelectNone => "Select None",
            Command::SelectPrevious => "Add the Previous Frame to the Selection",
            Command::SelectNext => "Add the Next Frame to the Selection",
            Command::Copy => "Copy Settings",
            Command::CopyChoose => "Choose Settings to Copy…",
            Command::Paste => "Paste Settings",
            Command::PastePrevious => "Paste Settings from Previous",
            Command::PasteStack => "Paste Settings onto the Stack",
            Command::ToggleStack => "Open Out or Close Up the Stack",
            Command::Crop => "Crop & Straighten",
            Command::RotateLeft => "Rotate Left",
            Command::RotateRight => "Rotate Right",
            Command::ResetCrop => "Reset the Crop",
            Command::AutoTone => "Auto Tone",
            Command::AutoWhiteBalance => "Auto White Balance",
            Command::AutoExposure => "Auto Exposure",
            Command::AutoLevel => "Auto Level",
            Command::Flat => "Flat Profile, or Camera Match",
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
            Command::Zoom => s(Modifiers::NONE, Key::Z),
            Command::SelectAll => s(CMD, Key::A),
            Command::SelectNone => s(CMD, Key::D),
            Command::SelectPrevious => s(Modifiers::SHIFT, Key::ArrowLeft),
            Command::SelectNext => s(Modifiers::SHIFT, Key::ArrowRight),
            Command::Copy => s(CMD, Key::C),
            Command::CopyChoose => s(CMD_SHIFT, Key::C),
            Command::Paste => s(CMD, Key::V),
            Command::PastePrevious => s(CMD_ALT, Key::V),
            // Lightroom has no stacks to paste onto; its own Ctrl+Shift+V
            // is the paste Ctrl+V is here.
            Command::PasteStack => s(CMD_SHIFT, Key::V),
            // Omacull's.
            Command::ToggleStack => s(Modifiers::NONE, Key::G),
            Command::Crop => s(Modifiers::NONE, Key::R),
            Command::RotateLeft => s(CMD, Key::OpenBracket),
            Command::RotateRight => s(CMD, Key::CloseBracket),
            Command::ResetCrop => s(CMD_ALT, Key::R),
            Command::AutoTone => s(CMD, Key::U),
            Command::AutoWhiteBalance => s(CMD_SHIFT, Key::U),
            // Lightroom has one Auto, on Ctrl+U; these two are buttons.
            Command::AutoExposure | Command::AutoLevel => None,
            // Lightroom has no key for a profile. Its F is Full Screen,
            // which is Hyprland's here.
            Command::Flat => s(Modifiers::NONE, Key::F),
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
    /// `v_down` remembers between frames whether V is held.
    pub fn pressed(ctx: &egui::Context, v_down: &mut bool) -> Vec<Command> {
        Self::pressed_with(ctx, v_down, crate::hotkeys::current())
    }

    /// [`Self::pressed`] with these shortcuts.
    pub fn pressed_with(ctx: &egui::Context, v_down: &mut bool, hotkeys: &crate::hotkeys::Hotkeys) -> Vec<Command> {
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
            // egui-winit turns Ctrl+C into a Copy event instead of a key
            // press. Ctrl+V becomes a Paste event only when the clipboard
            // holds text, but its V is still released: a V released
            // without being pressed was Ctrl+V. As Omapix reads them
            // (`omapix/src/commands.rs`).
            // The command among `commands` whose shortcut is `key` with
            // this Shift and Alt.
            let held = |commands: &[Command], key: Key, modifiers: Modifiers| {
                commands.iter().copied().find(|&c| {
                    hotkeys.command(c).is_some_and(|s| {
                        s.logical_key == key && s.modifiers.alt == modifiers.alt && s.modifiers.shift == modifiers.shift
                    })
                })
            };
            for event in &i.events {
                let command = match event {
                    egui::Event::Copy => held(&[Command::Copy, Command::CopyChoose], Key::C, i.modifiers),
                    egui::Event::Key { key: Key::V, pressed: down, modifiers, .. } => {
                        let pasted = !*down && !*v_down;
                        *v_down = *down;
                        let pastes = [Command::Paste, Command::PastePrevious, Command::PasteStack];
                        pasted.then(|| held(&pastes, Key::V, *modifiers)).flatten()
                    }
                    _ => None,
                };
                if let Some(command) = command.filter(|c| !pressed.contains(c)) {
                    pressed.push(command);
                }
            }
            i.events.retain(|e| !matches!(e, egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_)));
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
        let key = egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
        events(ctx, &mut false, modifiers, vec![key])
    }

    /// Commands pressed in a frame with these events and modifiers held.
    fn events(ctx: &egui::Context, v_down: &mut bool, modifiers: Modifiers, events: Vec<egui::Event>) -> Vec<Command> {
        let events = std::iter::once(egui::Event::ModifiersChanged(modifiers)).chain(events).collect();
        let input = egui::RawInput { events, ..Default::default() };
        let mut pressed = Vec::new();
        let mut output = ctx.run_ui(input, |ui| pressed = Command::pressed(ui.ctx(), v_down));
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
        assert_eq!(press(&ctx, Modifiers::NONE, Key::Z), [Command::Zoom]);
    }

    #[test]
    fn selecting_and_stacks() {
        let ctx = egui::Context::default();
        assert_eq!(press(&ctx, CMD, Key::A), [Command::SelectAll]);
        assert_eq!(press(&ctx, CMD, Key::D), [Command::SelectNone]);
        assert_eq!(press(&ctx, Modifiers::SHIFT, Key::ArrowLeft), [Command::SelectPrevious], "not mistaken for a step");
        assert_eq!(press(&ctx, Modifiers::SHIFT, Key::ArrowRight), [Command::SelectNext]);
        assert_eq!(press(&ctx, Modifiers::NONE, Key::G), [Command::ToggleStack]);
    }

    #[test]
    fn the_crop_tool_and_auto() {
        let ctx = egui::Context::default();
        assert_eq!(press(&ctx, Modifiers::NONE, Key::R), [Command::Crop]);
        assert_eq!(press(&ctx, CMD_SHIFT, Key::R), [Command::Reset], "not the crop's reset");
        assert_eq!(press(&ctx, CMD_ALT, Key::R), [Command::ResetCrop]);
        assert_eq!(press(&ctx, CMD, Key::OpenBracket), [Command::RotateLeft]);
        assert_eq!(press(&ctx, CMD, Key::CloseBracket), [Command::RotateRight]);
        assert_eq!(press(&ctx, CMD, Key::U), [Command::AutoTone], "not an unmark");
        assert_eq!(press(&ctx, CMD_SHIFT, Key::U), [Command::AutoWhiteBalance], "not mistaken for auto tone");
        assert_eq!(press(&ctx, Modifiers::NONE, Key::U), [Command::Unmark]);
    }

    #[test]
    fn copy_and_paste_arrive_as_egui_winit_sends_them() {
        let ctx = egui::Context::default();
        let mut v_down = false;
        let none = Modifiers::NONE;
        let v = |down: bool, modifiers: Modifiers| egui::Event::Key { key: Key::V, physical_key: None, pressed: down, repeat: false, modifiers };
        assert_eq!(events(&ctx, &mut v_down, CMD, vec![egui::Event::Copy]), [Command::Copy]);
        assert_eq!(events(&ctx, &mut v_down, CMD_SHIFT, vec![egui::Event::Copy]), [Command::CopyChoose]);

        // Ctrl+V with no text on the clipboard: only the release arrives,
        // even if Ctrl was let go first.
        assert_eq!(events(&ctx, &mut v_down, CMD, vec![]), []);
        assert_eq!(events(&ctx, &mut v_down, none, vec![v(false, none)]), [Command::Paste]);
        // With text on it, a Paste event comes first.
        assert_eq!(events(&ctx, &mut v_down, CMD, vec![egui::Event::Paste("text".into())]), []);
        assert_eq!(events(&ctx, &mut v_down, CMD, vec![v(false, CMD)]), [Command::Paste]);
        assert_eq!(events(&ctx, &mut v_down, CMD_ALT, vec![v(false, CMD_ALT)]), [Command::PastePrevious]);
        assert_eq!(events(&ctx, &mut v_down, CMD_SHIFT, vec![v(false, CMD_SHIFT)]), [Command::PasteStack]);

        // A V pressed and let go with no Ctrl pastes nothing.
        assert_eq!(events(&ctx, &mut v_down, none, vec![v(true, none)]), []);
        assert_eq!(events(&ctx, &mut v_down, none, vec![v(false, none)]), []);
    }
}
