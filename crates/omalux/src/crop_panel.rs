//! The Crop & Straighten tool's settings, in the develop panel under the
//! histogram while the tool is open, as Lightroom's tool drawer is: the
//! aspect and its lock, the angle, Auto, the quarter turns, Reset and
//! Close.
//!
//! What's in it follows Lightcraft's crop panel (`crop` in
//! `ui-egui/src/panels/right.rs` at `701f78e`), without its flips, custom
//! aspect and overlay chooser, in egui's own widgets.

use egui::{Button, ComboBox, FontId, TextStyle, Ui};
use lightcraft_develop::DevelopSettings;
use lightcraft_develop::controls;
use omalux_engine::crop::{self, Aspect};

use crate::commands::Command;
use crate::develop;
use crate::hotkeys::{self, format_shortcut};
use crate::slider::{SliderOut, slider};
use crate::theme::Theme;

/// What was done in the crop panel this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Out {
    /// An aspect was chosen from the menu.
    pub aspect: Option<Aspect>,
    /// The lock was clicked: locked, the crop keeps its shape when it's
    /// dragged.
    pub lock: bool,
    /// Swap the aspect, landscape to portrait and back (Lightroom's X).
    pub swap: bool,
    /// What happened to the Angle slider.
    pub angle: SliderOut,
    /// Auto was clicked: level the frame.
    pub level: bool,
    /// A quarter turn was asked for: clockwise if true.
    pub turn: Option<bool>,
    pub reset: bool,
    /// Close was clicked.
    pub done: bool,
}

/// The crop panel showing `settings`' crop of a frame `dims` in size as
/// it's shown. `locked` is whether the crop keeps its shape when dragged.
pub fn show(ui: &mut Ui, theme: &Theme, settings: &DevelopSettings, dims: (f64, f64), locked: bool) -> Out {
    let mut out = Out::default();
    develop::heading(ui, theme, "Crop & Straighten");
    ui.add_space(4.0);

    ui.scope(|ui| {
        ui.style_mut().text_styles.insert(TextStyle::Button, FontId::proportional(12.5));
        ui.style_mut().text_styles.insert(TextStyle::Body, FontId::proportional(12.5));

        // A ratio is the same ratio on its side (4 x 5 swapped is still 4 x 5).
        let current_aspect = crop::aspect(settings, dims);
        let is_current = |ratio: (u32, u32)| matches!(current_aspect, Some(Aspect::Ratio(w, h)) if (w, h) == ratio || (h, w) == ratio);
        let current_label = match current_aspect {
            Some(Aspect::Original) => "Original",
            _ => crop::RATIOS.iter().find(|(_, ratio)| is_current(*ratio)).map_or("Custom", |(label, _)| *label),
        };

        ui.horizontal(|ui| {
            ui.label("Aspect");
            ComboBox::from_id_salt("crop.aspect")
                .selected_text(current_label)
                .show_ui(ui, |ui| {
                    let is_orig = current_aspect == Some(Aspect::Original);
                    if ui.selectable_label(is_orig, "Original").clicked() {
                        out.aspect = Some(Aspect::Original);
                    }
                    for (label, ratio) in crop::RATIOS {
                        if ui.selectable_label(is_current(*ratio), *label).clicked() {
                            out.aspect = Some(Aspect::Ratio(ratio.0, ratio.1));
                        }
                    }
                });
            let lock_text = if locked { "Locked" } else { "Unlocked" };
            if ui
                .add(Button::new(lock_text).selected(locked))
                .on_hover_text("Locks the crop's shape when dragged (A)")
                .clicked()
            {
                out.lock = true;
            }
        });
        ui.add_space(4.0);

        if let Some(mut spec) = controls::find("crop.angle").copied() {
            spec.label = "Angle";
            out.angle = slider(ui, theme, &spec, settings.crop.geometry.angle, 0.0, false);
        }
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            if ui.button("Auto").on_hover_text("Levels the frame by its horizon or uprights").clicked() {
                out.level = true;
            }
        });
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            let rot_left_tip = match hotkeys::current().command(Command::RotateLeft) {
                Some(s) => format!("Rotate Left ({})", format_shortcut(&s)),
                None => "Rotate Left".to_owned(),
            };
            if ui.button("Rotate Left").on_hover_text(rot_left_tip).clicked() {
                out.turn = Some(false);
            }
            let rot_right_tip = match hotkeys::current().command(Command::RotateRight) {
                Some(s) => format!("Rotate Right ({})", format_shortcut(&s)),
                None => "Rotate Right".to_owned(),
            };
            if ui.button("Rotate Right").on_hover_text(rot_right_tip).clicked() {
                out.turn = Some(true);
            }
        });
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            if ui.button("Swap").on_hover_text("Turns the crop on its side (X)").clicked() {
                out.swap = true;
            }
            let reset_tip = match hotkeys::current().command(Command::ResetCrop) {
                Some(s) => format!("Reset ({})", format_shortcut(&s)),
                None => "Reset".to_owned(),
            };
            if ui.button("Reset").on_hover_text(reset_tip).clicked() {
                out.reset = true;
            }
            if ui.button("Close").on_hover_text("Return, or R").clicked() {
                out.done = true;
            }
        });
        ui.add_space(8.0);
    });

    out
}
