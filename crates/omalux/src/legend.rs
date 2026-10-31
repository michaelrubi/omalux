//! The keyboard shortcuts legend (Lightroom's Develop Shortcuts, Ctrl+/):
//! every command and the key it's on now, and the keys and clicks that
//! aren't commands.

use egui::{Align2, CornerRadius, FontId, Frame, Margin, RichText, Sense, Stroke, StrokeKind, Ui, vec2};

use crate::commands::Command;
use crate::hotkeys::{self, Hotkeys};
use crate::theme::Theme;

/// The commands, in sections, in order: each once.
const SECTIONS: &[(&str, &[Command])] = &[
    ("Frames", &[Command::Previous, Command::Next, Command::First, Command::Last, Command::Filter, Command::ToggleStack]),
    ("Marks", &[Command::Pick, Command::Reject, Command::Unmark, Command::Star1, Command::Star2, Command::Star3, Command::Star4, Command::Star5]),
    ("Selection", &[Command::SelectAll, Command::SelectNone, Command::SelectPrevious, Command::SelectNext]),
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
    ("Crop and Transform", &[Command::Crop, Command::RotateLeft, Command::RotateRight, Command::ResetCrop, Command::GuidedUpright, Command::SpotRemoval]),
    ("Copy and Paste", &[Command::Copy, Command::CopyChoose, Command::Paste, Command::PastePrevious, Command::PasteStack, Command::AutoSync]),
    ("View", &[Command::BeforeAfter, Command::CompareSideBySide, Command::CompareTopBottom, Command::CompareSplit, Command::Clipping, Command::Zoom, Command::InfoCycle, Command::InfoShow, Command::Shortcuts]),
    ("Folder", &[Command::Open, Command::Close, Command::Export, Command::ExportAndEdit, Command::Quit]),
];

/// The keys and clicks that aren't commands, as (key, what it does).
const OTHERS: &[(&str, &str)] = &[
    ("0", "Unmark"),
    ("Hold Z", "Look at 100% while held"),
    ("Click the frame", "Zoom to 100%, or back"),
    ("Drag or scroll", "Pan at 100%"),
    ("Double-click a slider", "Reset it"),
    ("Shift+double-click a slider", "Set it automatically"),
    ("Shift+drag a slider", "Fine adjustment"),
    ("Click a slider's value", "Type a value"),
    ("Right-click", "Menus on the frame and the filmstrip"),
    ("A panel's switch", "The frame without the panel"),
];

/// The crop tool's own keys, while it's open.
const CROP_TOOL: &[(&str, &str)] = &[("X", "Swap the aspect"), ("A", "Lock the aspect"), ("Return", "Done"), ("Esc", "Cancel"), ("Double-click the crop", "Done")];

/// The shortcut text to show for a command under these hotkeys.
fn key_text(hotkeys: &Hotkeys, command: Command) -> String {
    hotkeys.command(command).map_or_else(|| "—".to_owned(), |s| hotkeys::format_shortcut(&s))
}

/// A section heading inside a column.
fn section_heading(ui: &mut Ui, theme: &Theme, title: &str) {
    ui.add_space(4.0);
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    ui.painter().text(row.left_center() + vec2(2.0, 0.0), Align2::LEFT_CENTER, title, FontId::proportional(13.5), theme.dark_foreground);
    ui.add_space(2.0);
}

/// A row: what it does on the left, its key in a key cap on the right, or
/// a dimmed dash for none.
fn row(ui: &mut Ui, theme: &Theme, label: &str, key: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
    let painter = ui.painter();
    painter.text(rect.left_center() + vec2(2.0, 0.0), Align2::LEFT_CENTER, label, FontId::proportional(12.0), theme.foreground);
    if key == "—" {
        painter.text(rect.right_center() - vec2(4.0, 0.0), Align2::RIGHT_CENTER, key, FontId::proportional(12.0), theme.muted);
        return;
    }
    let galley = painter.layout_no_wrap(key.to_owned(), FontId::proportional(11.0), theme.foreground);
    let plate = Align2::RIGHT_CENTER.anchor_size(rect.right_center() - vec2(2.0, 0.0), galley.size() + vec2(8.0, 4.0));
    painter.rect_filled(plate, CornerRadius::same(2), theme.lighter_background);
    painter.rect_stroke(plate, CornerRadius::same(2), Stroke::new(1.0, theme.muted), StrokeKind::Inside);
    painter.galley(plate.center() - galley.size() / 2.0, galley, theme.foreground);
}

/// Every section, as rows of (label, key), the commands' with the keys in use.
fn sections(hotkeys: &Hotkeys) -> Vec<(&'static str, Vec<(&'static str, String)>)> {
    let commands = SECTIONS.iter().map(|(title, commands)| (*title, commands.iter().map(|&c| (c.label(), key_text(hotkeys, c))).collect()));
    let others = [("Mouse and other keys", OTHERS), ("In the crop tool", CROP_TOOL)];
    let others = others.into_iter().map(|(title, rows)| (title, rows.iter().map(|&(key, label)| (label, key.to_owned())).collect()));
    commands.chain(others).collect()
}

/// `sections` cut into `columns` runs in order, the tallest as short as
/// it can be (each section is its rows and two for its title and gap).
fn split<T>(sections: &[(&str, Vec<T>)], columns: usize) -> Vec<std::ops::Range<usize>> {
    let sizes: Vec<usize> = sections.iter().map(|(_, rows)| rows.len() + 2).collect();
    let height = |run: &std::ops::Range<usize>| sizes[run.clone()].iter().sum::<usize>();
    // The best cut of sections[start..] into `columns` runs: there are
    // only a dozen sections, so every cut is tried.
    fn best(sizes: &[usize], start: usize, columns: usize, height: &dyn Fn(&std::ops::Range<usize>) -> usize) -> Vec<std::ops::Range<usize>> {
        if columns <= 1 || start >= sizes.len() {
            return std::iter::once(start..sizes.len()).collect();
        }
        (start + 1..=sizes.len())
            .map(|end| {
                let mut runs: Vec<_> = std::iter::once(start..end).collect();
                runs.extend(best(sizes, end, columns - 1, height));
                runs
            })
            .min_by_key(|runs| runs.iter().map(height).max().unwrap_or(0))
            .unwrap_or_else(|| std::iter::once(start..sizes.len()).collect())
    }
    let mut runs = best(&sizes, 0, columns, &height);
    // A column left empty at the end still counts as one.
    runs.resize_with(columns, || sizes.len()..sizes.len());
    runs
}

/// Draw the legend over everything. Returns whether it stays open.
pub fn show(ctx: &egui::Context, theme: &Theme) -> bool {
    let sections = sections(hotkeys::current());
    let screen = ctx.content_rect();
    let columns: usize = if screen.width() >= 1050.0 { 3 } else if screen.width() >= 700.0 { 2 } else { 1 };
    let (column, gap) = (320.0, 20.0);
    let width = (columns as f32 * column + (columns - 1) as f32 * gap).min(screen.width() - 40.0);
    let frame = Frame::new().fill(theme.background).stroke(Stroke::new(1.0, theme.muted)).inner_margin(Margin::symmetric(20, 16));
    let modal = egui::Modal::new(egui::Id::new("keyboard shortcuts")).frame(frame).show(ctx, |ui| {
        ui.set_width(width);
        ui.label(RichText::new("Keyboard Shortcuts").strong().size(15.0).color(theme.foreground));
        ui.add_space(8.0);
        let scroll = egui::ScrollArea::vertical().auto_shrink([false, true]).max_height((screen.height() - 100.0).max(200.0));
        scroll.show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = gap;
            ui.columns(columns, |cols| {
                for (ui, run) in cols.iter_mut().zip(split(&sections, columns)) {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for (title, rows) in &sections[run] {
                        section_heading(ui, theme, title);
                        for (label, key) in rows {
                            row(ui, theme, label, key);
                        }
                        ui.add_space(8.0);
                    }
                }
            });
        });
    });
    !modal.should_close()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use crate::hotkeys::parse_shortcut;

    #[test]
    fn every_command_in_command_all_is_in_exactly_one_section() {
        for &command in Command::ALL {
            let count = SECTIONS.iter().flat_map(|(_, commands)| commands.iter()).filter(|&&c| c == command).count();
            assert_eq!(count, 1, "{command:?} is in {count} sections");
        }
    }

    #[test]
    fn the_columns_take_every_section_in_order_and_about_as_many_rows_each() {
        let sections = sections(&Hotkeys::default());
        for columns in 1..=3 {
            let runs = split(&sections, columns);
            assert_eq!(runs.len(), columns);
            assert!(runs.windows(2).all(|w| w[0].end == w[1].start) && runs[0].start == 0 && runs[columns - 1].end == sections.len());
        }
        // As even as whole sections allow: no column longer than its share
        // and the longest section.
        let size = |(_, rows): &(&str, Vec<_>)| rows.len() + 2;
        let rows = |run: std::ops::Range<usize>| sections[run].iter().map(size).sum::<usize>();
        let (total, longest) = (rows(0..sections.len()), sections.iter().map(size).max().unwrap());
        let lengths: Vec<usize> = split(&sections, 3).into_iter().map(rows).collect();
        assert!(*lengths.iter().max().unwrap() <= total.div_ceil(3) + longest, "{lengths:?}");
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
