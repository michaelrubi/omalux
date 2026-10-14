//! The keyboard shortcuts legend (Lightroom's Develop Shortcuts, Ctrl+/):
//! every command and the key it's on now, and the keys and clicks that
//! aren't commands.

use egui::{Align2, CornerRadius, FontId, Frame, Margin, RichText, Sense, Stroke, StrokeKind, Ui, vec2};

use crate::commands::Command;
use crate::hotkeys::{self, Hotkeys};
use crate::theme::Theme;

/// Every command group shown in the legend, in order.
pub const SECTIONS: &[(&str, &[Command])] = &[
    (
        "Frames",
        &[
            Command::Previous,
            Command::Next,
            Command::First,
            Command::Last,
            Command::Filter,
            Command::ToggleStack,
        ],
    ),
    (
        "Marks",
        &[
            Command::Pick,
            Command::Reject,
            Command::Unmark,
            Command::Star1,
            Command::Star2,
            Command::Star3,
            Command::Star4,
            Command::Star5,
        ],
    ),
    (
        "Selection",
        &[
            Command::SelectAll,
            Command::SelectNone,
            Command::SelectPrevious,
            Command::SelectNext,
        ],
    ),
    (
        "Develop",
        &[
            Command::SliderPrevious,
            Command::SliderNext,
            Command::Increase,
            Command::Decrease,
            Command::IncreaseMore,
            Command::DecreaseMore,
            Command::WhiteBalance,
            Command::Flat,
            Command::AutoTone,
            Command::AutoWhiteBalance,
            Command::AutoExposure,
            Command::AutoLevel,
            Command::Reset,
            Command::Undo,
            Command::Redo,
        ],
    ),
    (
        "Crop",
        &[
            Command::Crop,
            Command::RotateLeft,
            Command::RotateRight,
            Command::ResetCrop,
        ],
    ),
    (
        "Copy and Paste",
        &[
            Command::Copy,
            Command::CopyChoose,
            Command::Paste,
            Command::PastePrevious,
            Command::PasteStack,
        ],
    ),
    (
        "View",
        &[
            Command::BeforeAfter,
            Command::Clipping,
            Command::Zoom,
            Command::Shortcuts,
        ],
    ),
    (
        "Folder",
        &[
            Command::Open,
            Command::Close,
            Command::Export,
            Command::ExportAndEdit,
            Command::Quit,
        ],
    ),
];

const MOUSE_SECTION_TITLE: &str = "Mouse and other keys";

const MOUSE_AND_OTHER_KEYS: &[&str] = &[
    "\"0\" — Unmark",
    "\"Hold Z\" — Look at 100% while held",
    "\"Click the frame\" — Zoom to 100%, or back",
    "\"Drag or scroll\" — Pan at 100%",
    "\"Double-click a slider\" — Reset it",
    "\"Shift+double-click a slider\" — Set it automatically",
    "\"Shift+drag a slider\" — Fine adjustment",
    "\"Click a slider's value\" — Type a value",
    "\"Right-click\" — Menus on the frame and the filmstrip",
    "In the crop tool: \"X\" — Swap the aspect, \"A\" — Lock the aspect, \"Return\" — Done, \"Esc\" — Cancel, \"Double-click the crop\" — Done",
];

/// The shortcut text to show for a command under these hotkeys.
pub(crate) fn key_text(hotkeys: &Hotkeys, command: Command) -> String {
    hotkeys
        .command(command)
        .map_or_else(|| "—".to_owned(), |s| crate::hotkeys::format_shortcut(&s))
}

/// A section heading inside a column.
fn section_heading(ui: &mut Ui, theme: &Theme, title: &str) {
    ui.add_space(4.0);
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    ui.painter().text(
        row.left_center() + vec2(2.0, 0.0),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(13.5),
        theme.dark_foreground,
    );
    ui.add_space(2.0);
}

/// A command row: label on the left, shortcut keycap or dimmed dash on the right.
fn command_row(ui: &mut Ui, theme: &Theme, hotkeys: &Hotkeys, command: Command) {
    let text = key_text(hotkeys, command);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
    let painter = ui.painter();

    painter.text(
        rect.left_center() + vec2(2.0, 0.0),
        Align2::LEFT_CENTER,
        command.label(),
        FontId::proportional(12.0),
        theme.foreground,
    );

    if text == "—" {
        painter.text(
            rect.right_center() - vec2(4.0, 0.0),
            Align2::RIGHT_CENTER,
            "—",
            FontId::proportional(12.0),
            theme.muted,
        );
    } else {
        let key_galley = painter.layout_no_wrap(text, FontId::proportional(11.0), theme.foreground);
        let plate_size = key_galley.size() + vec2(8.0, 4.0);
        let plate_rect = Align2::RIGHT_CENTER.anchor_size(rect.right_center() - vec2(2.0, 0.0), plate_size);
        painter.rect_filled(plate_rect, CornerRadius::same(2), theme.lighter_background);
        painter.rect_stroke(plate_rect, CornerRadius::same(2), Stroke::new(1.0, theme.muted), StrokeKind::Inside);
        let text_pos = plate_rect.center() - key_galley.size() / 2.0;
        painter.galley(text_pos, key_galley, theme.foreground);
    }
}

/// The sections assigned to column `col_idx` when dividing into `num_columns`.
fn column_sections(num_columns: usize, col_idx: usize) -> &'static [usize] {
    match (num_columns, col_idx) {
        (3, 0) => &[0, 1, 2],
        (3, 1) => &[3, 4],
        (3, 2) => &[5, 6, 7, 8],
        (2, 0) => &[0, 1, 2, 3],
        (2, 1) => &[4, 5, 6, 7, 8],
        _ => &[0, 1, 2, 3, 4, 5, 6, 7, 8],
    }
}

/// Draw one section (or the mouse gestures section).
fn show_section(ui: &mut Ui, theme: &Theme, hotkeys: &Hotkeys, section_idx: usize) {
    if section_idx < SECTIONS.len() {
        let (title, commands) = SECTIONS[section_idx];
        section_heading(ui, theme, title);
        for &command in commands {
            command_row(ui, theme, hotkeys, command);
        }
    } else {
        section_heading(ui, theme, MOUSE_SECTION_TITLE);
        for &item in MOUSE_AND_OTHER_KEYS {
            ui.add(egui::Label::new(RichText::new(item).size(12.0).color(theme.foreground)).wrap());
        }
    }
}

/// Draw the legend over everything. Returns whether it stays open.
pub fn show(ctx: &egui::Context, theme: &Theme) -> bool {
    let hotkeys = hotkeys::current();
    let screen_rect = ctx.content_rect();
    let screen_width = screen_rect.width();
    let screen_height = screen_rect.height();

    let num_columns: usize = if screen_width >= 1050.0 {
        3
    } else if screen_width >= 700.0 {
        2
    } else {
        1
    };

    let col_width = 320.0;
    let col_gap = 20.0;
    let desired_width = num_columns as f32 * col_width + (num_columns.saturating_sub(1) as f32) * col_gap;
    let modal_width = desired_width.min(screen_width - 40.0);

    let frame = Frame::new()
        .fill(theme.background)
        .stroke(Stroke::new(1.0, theme.muted))
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(Margin::symmetric(20, 16));

    let modal = egui::Modal::new(egui::Id::new("keyboard shortcuts"))
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(modal_width);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Keyboard Shortcuts").strong().size(15.0).color(theme.foreground));
            });
            ui.add_space(8.0);
            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .max_height((screen_height - 100.0).max(200.0))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = col_gap;
                    ui.columns(num_columns, |cols| {
                        for (col_idx, col_ui) in cols.iter_mut().enumerate() {
                            col_ui.spacing_mut().item_spacing.y = 2.0;
                            let sections = column_sections(num_columns, col_idx);
                            for (i, &sec_idx) in sections.iter().enumerate() {
                                if i > 0 {
                                    col_ui.add_space(8.0);
                                }
                                show_section(col_ui, theme, hotkeys, sec_idx);
                            }
                        }
                    });
                });
        });

    if modal.should_close() {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use crate::hotkeys::parse_shortcut;

    #[test]
    fn every_command_in_command_all_is_in_exactly_one_section() {
        for &cmd in Command::ALL {
            let count = SECTIONS
                .iter()
                .flat_map(|(_, cmds)| cmds.iter())
                .filter(|&&c| c == cmd)
                .count();
            assert_eq!(
                count, 1,
                "{cmd:?} should appear in exactly one section, but appeared {count} times"
            );
        }
        let total_in_sections: usize = SECTIONS.iter().map(|(_, cmds)| cmds.len()).sum();
        assert_eq!(total_in_sections, Command::ALL.len());
    }

    #[test]
    fn the_shortcut_shown_is_the_one_in_use() {
        let mut overrides = HashMap::new();
        overrides.insert(Command::Open, parse_shortcut("Ctrl+Shift+O").unwrap());
        overrides.insert(Command::Quit, None);
        let hotkeys = Hotkeys::from_overrides(overrides);

        assert_eq!(key_text(&hotkeys, Command::Open), "Ctrl+Shift+O");
        assert_eq!(key_text(&hotkeys, Command::Quit), "—");
        assert_eq!(key_text(&hotkeys, Command::Previous), "Left");
        assert_eq!(key_text(&hotkeys, Command::AutoExposure), "—");
    }

    #[test]
    fn rendering_doesnt_panic() {
        let ctx = egui::Context::default();
        let theme = Theme::default();
        let input = egui::RawInput::default();
        let mut open = false;
        let mut output = ctx.run_ui(input, |ui| {
            open = show(ui.ctx(), &theme);
        });
        output.textures_delta.clear();
        assert!(open, "modal starts open");
    }

    #[test]
    fn modal_closes_on_escape() {
        let ctx = egui::Context::default();
        let theme = Theme::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            assert!(show(ui.ctx(), &theme));
        });
        output.textures_delta.clear();

        let events = vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }];
        let input = egui::RawInput { events, ..Default::default() };
        let mut open = true;
        let mut output = ctx.run_ui(input, |ui| {
            open = show(ui.ctx(), &theme);
        });
        output.textures_delta.clear();
        assert!(!open, "modal closes on escape");
    }
}
