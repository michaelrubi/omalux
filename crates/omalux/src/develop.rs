//! The develop panel's sliders, in Lightroom's order under Lightroom's
//! names: the Profile, White Balance, Light, HDR, Presence, Detail, Lens
//! Corrections. Which controls there are, their ranges and what they do
//! are Lightcraft's (`lightcraft_develop::controls`), all but HDR, which
//! is OmaLux's (`omalux_engine::look`).
//!
//! Each panel but the Profile has Lightroom's switch on its heading, which
//! shows the frame without the panel's settings while it's off. It's a
//! look, not an edit: nothing's saved, and touching one of its sliders
//! turns the panel back on, as in Lightroom.

use egui::{Align2, FontId, Sense, Ui, vec2};
use lightcraft_develop::controls::{self, ControlSpec};
use lightcraft_develop::{DevelopSettings, WbMode};
use omalux_engine::auto::Auto;
use omalux_engine::copy::Group;
use omalux_engine::look::{Camera, Profile};

use crate::commands::Command;
use crate::hotkeys::{self, format_shortcut};
use crate::slider::{SliderOut, slider};
use crate::theme::Theme;

/// A row of a panel.
enum Row {
    /// A slider, by its control's id, under Lightcraft's label.
    Slider(&'static str),
    /// A slider under Lightroom's name, where Lightcraft's label is
    /// another (its Amount is "Sharpening", and two of its Detail
    /// controls are both "Detail").
    Named(&'static str, &'static str),
    /// A heading inside a panel, as Lightroom's Detail has.
    Sub(&'static str),
    /// The Remove Chromatic Aberration checkbox.
    RemoveCa,
    /// The Enable Profile Corrections checkbox, and the lens the profile
    /// is for.
    ProfileCorrections,
}

use Row::{Named, ProfileCorrections, RemoveCa, Slider, Sub};

/// The panels and their rows, top to bottom: the order `,` and `.` step
/// through the sliders in. Lightcraft's other noise-reduction controls
/// aren't shown. Lightroom draws each of Defringe's hue ranges as one
/// two-thumbed slider, which neither Lightcraft nor OmaLux has: they're
/// two sliders each here, under Lightcraft's labels.
const PANELS: &[(&str, &[Row])] = &[
    // Where Lightroom's Basic panel has it: above the white balance.
    ("Profile", &[]),
    ("White Balance", &[Slider("wb.temp"), Slider("wb.tint")]),
    (
        "Light",
        &[
            Slider("light.exposure"),
            Slider("light.contrast"),
            Slider("light.highlights"),
            Slider("light.shadows"),
            Slider("light.whites"),
            Slider("light.blacks"),
        ],
    ),
    // Its Highlights and Shadows move either end of the range off the
    // Amount, so one can have more of it than the other.
    ("HDR", &[Named("light.hdr", "Amount"), Named("light.hdrHighlights", "Highlights"), Named("light.hdrShadows", "Shadows")]),
    (
        "Presence",
        &[Slider("effects.texture"), Slider("effects.clarity"), Slider("effects.dehaze"), Slider("color.vibrance"), Slider("color.saturation")],
    ),
    (
        "Detail",
        &[
            Sub("Sharpening"),
            Named("detail.sharpenAmount", "Amount"),
            Slider("detail.sharpenRadius"),
            Slider("detail.sharpenDetail"),
            Slider("detail.sharpenMasking"),
            Sub("Noise Reduction"),
            Named("detail.nrLuminance", "Luminance"),
            Named("detail.nrColor", "Color"),
        ],
    ),
    (
        "Lens Corrections",
        &[
            RemoveCa,
            ProfileCorrections,
            Sub("Amount"),
            Named("optics.profileDistortion", "Distortion"),
            Named("optics.profileVignetting", "Vignetting"),
            Sub("Defringe"),
            Slider("optics.defringePurple"),
            Slider("optics.defringePurpleHueLo"),
            Slider("optics.defringePurpleHueHi"),
            Slider("optics.defringeGreen"),
            Slider("optics.defringeGreenHueLo"),
            Slider("optics.defringeGreenHueHi"),
        ],
    ),
];

impl Row {
    /// The row's control as it's shown, if it's a slider.
    fn spec(&self) -> Option<ControlSpec> {
        match self {
            Slider(id) => controls::find(id).copied(),
            Named(id, label) => controls::find(id).map(|spec| ControlSpec { label, ..*spec }),
            Sub(_) | RemoveCa | ProfileCorrections => None,
        }
    }
}

/// The groups of settings a panel's switch turns off: none for the
/// Profile, which has no switch.
pub fn switches(title: &str) -> &'static [Group] {
    match title {
        "White Balance" => &[Group::WhiteBalance],
        "Light" => &[Group::Light],
        "HDR" => &[Group::Hdr],
        "Presence" => &[Group::Presence],
        "Detail" => &[Group::Sharpening, Group::NoiseReduction],
        "Lens Corrections" => &[Group::LensCorrections],
        _ => &[],
    }
}

/// The groups of the panel the slider at `index` in [`sliders`] is in.
pub fn switches_of(index: usize) -> &'static [Group] {
    let mut panels = PANELS.iter().flat_map(|(title, rows)| rows.iter().filter(|row| row.spec().is_some()).map(move |_| switches(title)));
    panels.nth(index).unwrap_or(&[])
}

/// Every slider's control, in order, as it's shown.
pub fn sliders() -> impl Iterator<Item = ControlSpec> {
    PANELS.iter().flat_map(|(_, rows)| rows.iter()).filter_map(Row::spec)
}

/// What was done in the panel this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PanelOut {
    /// The slider touched, by its place in [`sliders`], and what happened
    /// to it.
    pub slider: Option<(usize, SliderOut)>,
    /// The profile was clicked: the one to go to.
    pub profile: Option<Profile>,
    /// A white balance was chosen from the WB menu: As Shot, Auto or a
    /// preset.
    pub wb: Option<WbMode>,
    /// An Auto button was clicked.
    pub auto: Option<Auto>,
    /// Remove Chromatic Aberration was ticked, or unticked.
    pub remove_ca: Option<bool>,
    /// Enable Profile Corrections was ticked, or unticked.
    pub lens_profile: Option<bool>,
    /// A panel's switch was flipped, or one of its sliders touched while
    /// it was off: its groups, and whether they're on now.
    pub switch: Option<(&'static [Group], bool)>,
}

/// A panel's title, and the row it's in, for what goes at its right.
pub fn heading(ui: &mut Ui, theme: &Theme, title: &str) -> egui::Rect {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
    ui.painter().text(row.left_center() + vec2(4.0, 2.0), Align2::LEFT_CENTER, title, FontId::proportional(13.5), theme.dark_foreground);
    row
}

/// A panel's title with its switch at the left, as Lightroom draws it,
/// and the row it's in. True in the second if the switch was clicked.
fn switched_heading(ui: &mut Ui, theme: &Theme, title: &str, on: bool) -> (egui::Rect, bool) {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
    let switch = egui::Rect::from_center_size(row.left_center() + vec2(12.0, 2.0), vec2(16.0, 9.0));
    let clicked = ui.interact(switch.expand(4.0), ui.id().with(("switch", title)), Sense::click()).on_hover_text(if on { "Turn the panel off" } else { "Turn the panel on" }).clicked();
    let painter = ui.painter();
    let knob = if on { switch.right_center() - vec2(4.5, 0.0) } else { switch.left_center() + vec2(4.5, 0.0) };
    painter.rect_filled(switch, 4.5, if on { theme.accent } else { theme.darker_background });
    painter.rect_stroke(switch, 4.5, egui::Stroke::new(1.0, theme.muted), egui::StrokeKind::Inside);
    painter.circle_filled(knob, 3.0, if on { theme.dark_background } else { theme.muted });
    let colour = if on { theme.dark_foreground } else { theme.muted };
    painter.text(switch.right_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, title, FontId::proportional(13.5), colour);
    (row, clicked)
}

/// A heading inside a panel: smaller and dimmer than the panel's own.
fn sub_heading(ui: &mut Ui, theme: &Theme, title: &str) {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
    ui.painter().text(row.left_center() + vec2(4.0, 1.0), Align2::LEFT_CENTER, title, FontId::proportional(11.5), theme.muted);
}

/// What a command's button says when it's hovered: its name and its key.
fn tip(command: Command) -> String {
    let key = hotkeys::current().command(command).map(|s| format!(" ({})", format_shortcut(&s))).unwrap_or_default();
    format!("{}{key}", command.label())
}

/// A word to click at the right of a heading's row, ending at `right`,
/// which it moves left of itself for the next. `clickable` or not, it
/// takes its room.
fn word(ui: &mut Ui, right: &mut egui::Pos2, text: &str, colour: egui::Color32, clickable: Option<&str>) -> bool {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), FontId::proportional(12.5), colour);
    let rect = Align2::RIGHT_CENTER.anchor_size(*right, galley.size());
    right.x = rect.left() - 10.0;
    ui.painter().galley(rect.min, galley, colour);
    let Some(hover) = clickable else { return false };
    ui.interact(rect.expand(3.0), ui.id().with(text), Sense::click()).on_hover_text(hover).clicked()
}

/// Lightroom's WB menu, on the word naming the white balance: As Shot,
/// Auto and the presets. Custom is what Temp and Tint make, so it's only
/// ever shown, not chosen. The mode chosen, if one was.
fn wb_menu(ui: &mut Ui, theme: &Theme, right: &mut egui::Pos2, mode: WbMode) -> Option<WbMode> {
    let galley = ui.painter().layout_no_wrap(format!("{} ▾", mode.label()), FontId::proportional(12.5), theme.foreground);
    let rect = Align2::RIGHT_CENTER.anchor_size(*right, galley.size());
    right.x = rect.left() - 10.0;
    ui.painter().galley(rect.min, galley, theme.foreground);
    let response = ui.interact(rect.expand(3.0), ui.id().with("wb.menu"), Sense::click()).on_hover_text("White balance");
    let mut chosen = None;
    egui::Popup::menu(&response).show(|ui| {
        for choice in WbMode::ALL.into_iter().filter(|choice| *choice != WbMode::Custom) {
            if ui.selectable_label(choice == mode, choice.label()).clicked() {
                chosen = Some(choice);
            }
        }
    });
    chosen.filter(|chosen| *chosen != mode)
}

/// The sliders showing `settings`. `fresh` is what a double-click resets
/// each to; `selected` is the slider the keys act on; `camera` is what
/// the frame's camera did, which names its profile; `off` is the groups
/// switched off.
pub fn show(ui: &mut Ui, theme: &Theme, settings: &DevelopSettings, fresh: &DevelopSettings, selected: usize, camera: &Camera, off: &[Group]) -> PanelOut {
    let mut out = PanelOut::default();
    let mut index = 0;
    for (title, rows) in PANELS {
        let groups = switches(title);
        let on = !groups.iter().any(|group| off.contains(group));
        let row = if groups.is_empty() {
            heading(ui, theme, title)
        } else {
            let (row, clicked) = switched_heading(ui, theme, title, on);
            if clicked {
                out.switch = Some((groups, !on));
            }
            row
        };
        let before = out;
        let mut right = row.right_center() + vec2(-4.0, 2.0);
        match *title {
            "Profile" => {
                // Lightroom's "Profile: Camera Standard", with a click
                // where it has a menu: there are two to choose from.
                let profile = Profile::of(settings);
                if word(ui, &mut right, &profile.label(camera), theme.foreground, Some(&tip(Command::Flat))) {
                    out.profile = Some(profile.toggled());
                }
            }
            "White Balance" => {
                // Lightroom's "WB: As Shot": what the white balance is, a
                // click on it for its menu; and Auto beside it.
                out.wb = wb_menu(ui, theme, &mut right, settings.wb.mode);
                if settings.wb.mode != WbMode::Auto && word(ui, &mut right, "Auto", theme.foreground, Some(&tip(Command::AutoWhiteBalance))) {
                    out.auto = Some(Auto::WhiteBalance);
                }
            }
            "Light" => {
                if word(ui, &mut right, "Auto Tone", theme.foreground, Some(&tip(Command::AutoTone))) {
                    out.auto = Some(Auto::Tone);
                }
                if word(ui, &mut right, "Auto Exposure", theme.foreground, Some(&tip(Command::AutoExposure))) {
                    out.auto = Some(Auto::Exposure);
                }
            }
            _ => {}
        }
        for row in *rows {
            match row {
                Sub(title) => sub_heading(ui, theme, title),
                RemoveCa => {
                    let mut ticked = settings.optics.remove_ca;
                    if ui.checkbox(&mut ticked, "Remove Chromatic Aberration").changed() {
                        out.remove_ca = Some(ticked);
                    }
                }
                ProfileCorrections => {
                    let mut ticked = settings.optics.lens_profile;
                    if ui.checkbox(&mut ticked, "Enable Profile Corrections").changed() {
                        out.lens_profile = Some(ticked);
                    }
                    // Lightroom's Lens Profile: which lens it found, or that
                    // it found none (and the checkbox does nothing).
                    let lens = camera.lens.as_deref().unwrap_or("No profile for this lens");
                    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
                    ui.painter().text(row.left_center() + vec2(4.0, 1.0), Align2::LEFT_CENTER, lens, FontId::proportional(11.5), theme.muted);
                }
                Slider(id) | Named(id, _) => {
                    let Some(spec) = row.spec() else { continue };
                    let value = controls::get(settings, id).unwrap_or(spec.default);
                    let reset_to = controls::get(fresh, id).unwrap_or(spec.default);
                    let slid = slider(ui, theme, &spec, value, reset_to, index == selected);
                    if slid != SliderOut::default() {
                        out.slider = Some((index, slid));
                    }
                    index += 1;
                }
            }
        }
        // Anything done in a panel that's off turns it on.
        if !on && out.switch.is_none() && out != before {
            out.switch = Some((groups, true));
        }
        ui.add_space(6.0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_slider_is_in_its_panels_switch() {
        let of = |id| switches_of(sliders().position(|spec| spec.id == id).unwrap());
        assert_eq!(of("wb.tint"), [Group::WhiteBalance]);
        assert_eq!(of("light.hdrShadows"), [Group::Hdr]);
        assert_eq!(of("effects.dehaze"), [Group::Presence]);
        assert_eq!(of("detail.nrColor"), [Group::Sharpening, Group::NoiseReduction]);
        assert_eq!(of("optics.defringeGreenHueHi"), [Group::LensCorrections]);
        assert!(switches_of(sliders().count()).is_empty());
    }

        #[test]
    fn the_sliders_are_lightrooms_in_lightrooms_order() {
        let labels: Vec<&str> = sliders().map(|spec| spec.label).collect();
        assert_eq!(labels, [
            "Temp", "Tint",
            "Exposure", "Contrast", "Highlights", "Shadows", "Whites", "Blacks",
            "Amount", "Highlights", "Shadows",
            "Texture", "Clarity", "Dehaze", "Vibrance", "Saturation",
            "Amount", "Radius", "Detail", "Masking", "Luminance", "Color",
            "Distortion", "Vignetting",
            "Purple Amount", "Purple Hue Low", "Purple Hue High", "Green Amount", "Green Hue Low", "Green Hue High",
        ]);
    }
}
