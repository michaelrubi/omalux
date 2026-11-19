//! The develop panel's sliders, in Lightroom's order under Lightroom's
//! names: the Profile, White Balance, Light, HDR, Presence, Tone Curve,
//! HSL / Color, Color Grading, Detail, Lens Corrections, Transform. Which controls there are, their ranges and what they do
//! are Lightcraft's (`lightcraft_develop::controls`), all but HDR, which
//! is OmaLux's (`omalux_engine::look`).
//!
//! Each panel but the Profile has Lightroom's switch on its heading, which
//! shows the frame without the panel's settings while it's off. It's a
//! look, not an edit: nothing's saved, and touching one of its sliders
//! turns the panel back on, as in Lightroom.
//!
//! Tone Curve, HSL and Color Grading show one view at a time, as
//! Lightroom's do (a channel, a tab, a wheel): the sliders of the others
//! are hidden, and `,` and `.` step over them.

use egui::{Align2, FontId, Sense, Ui, vec2};
use lightcraft_develop::controls::{self, ControlSpec};
use lightcraft_develop::{DevelopSettings, Upright, WbMode};
use omalux_engine::auto::Auto;
use omalux_engine::copy::Group;
use omalux_engine::look::{Camera, Profile};

use lightcraft_raster::Histogram;

use crate::commands::Command;
use crate::curve::{self, Channel};
use crate::hotkeys::{self, format_shortcut};
use crate::slider::{SliderOut, slider};
use crate::theme::Theme;
use crate::wheel::{self, Which};

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
    /// Upright's buttons: Off, Auto, Guided, Level, Vertical, Full.
    UprightModes,
    /// The Constrain Crop checkbox.
    ConstrainCrop,
    /// Tone Curve's channel buttons and graph.
    Curve,
    /// HSL's tabs: Hue, Saturation, Luminance, All.
    HslTabs,
    /// A heading in HSL's All tab.
    HslSub(Hsl),
    /// Color Grading's views, and its wheels.
    Wheels,
}

use Row::{ConstrainCrop, Curve, HslSub, HslTabs, Named, ProfileCorrections, RemoveCa, Slider, Sub, UprightModes, Wheels};

/// HSL's tabs: one attribute of the eight colours, or all of them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Hsl {
    #[default]
    Hue,
    Saturation,
    Luminance,
    All,
}

impl Hsl {
    const TABS: [Hsl; 4] = [Hsl::Hue, Hsl::Saturation, Hsl::Luminance, Hsl::All];

    fn label(self) -> &'static str {
        match self {
            Hsl::Hue => "Hue",
            Hsl::Saturation => "Saturation",
            Hsl::Luminance => "Luminance",
            Hsl::All => "All",
        }
    }

    /// Whether the tab shows a mixer control's attribute (`hue`, `sat`, `lum`).
    fn shows(self, attribute: &str) -> bool {
        match self {
            Hsl::Hue => attribute == "hue",
            Hsl::Saturation => attribute == "sat",
            Hsl::Luminance => attribute == "lum",
            Hsl::All => true,
        }
    }
}

/// Color Grading's views: Lightroom's 3-Way, or one wheel large.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Grading {
    #[default]
    ThreeWay,
    One(Which),
}

impl Grading {
    const VIEWS: [Grading; 5] = [Grading::ThreeWay, Grading::One(Which::Shadows), Grading::One(Which::Midtones), Grading::One(Which::Highlights), Grading::One(Which::Global)];

    fn label(self) -> &'static str {
        match self {
            Grading::ThreeWay => "3-Way",
            Grading::One(which) => which.label(),
        }
    }
}

/// Which view each of those panels shows. Not an edit: kept while OmaLux
/// is open, for every frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Views {
    pub curve: Channel,
    pub hsl: Hsl,
    pub grading: Grading,
}

/// Whether the control `id` is in its panel's view.
fn shown(id: &str, views: &Views) -> bool {
    let mut parts = id.split('.');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("curve"), Some("refineSaturation"), _) => views.curve != Channel::Parametric,
        (Some("curve"), Some("highlights" | "lights" | "darks" | "shadows"), _) => views.curve == Channel::Parametric,
        (Some("mixer"), Some(_), Some(attribute)) => views.hsl.shows(attribute),
        (Some("grading"), Some(key), Some(part)) => match views.grading {
            Grading::One(which) => which.label().eq_ignore_ascii_case(key),
            // 3-Way: each wheel's luminance under it; Global has none.
            Grading::ThreeWay => part == "lum" && key != "global",
        },
        _ => true,
    }
}

/// Whether the slider at `index` in [`sliders`] is shown.
pub fn visible(index: usize, views: &Views) -> bool {
    sliders().nth(index).is_some_and(|spec| shown(spec.id, views))
}

/// The shown slider `steps` on from `index` (back, if negative), or the
/// last one that way: hidden ones are passed over.
pub fn step(index: usize, steps: isize, views: &Views) -> usize {
    let shown: Vec<usize> = (0..sliders().count()).filter(|&i| visible(i, views)).collect();
    let Some(&first) = shown.first() else { return index };
    let at = shown.iter().position(|&i| i >= index).unwrap_or(shown.len() - 1);
    // A hidden one counts as just before the next shown.
    let at = if shown[at] == index || steps < 0 { at as isize } else { at as isize - 1 };
    let to = (at + steps).clamp(0, shown.len() as isize - 1);
    shown.get(to as usize).copied().unwrap_or(first)
}

/// Upright's modes, as Lightroom's buttons are, left to right.
const UPRIGHTS: [(Upright, &str); 6] = [
    (Upright::Off, "Off"),
    (Upright::Auto, "Auto"),
    (Upright::Guided, "Guided"),
    (Upright::Level, "Level"),
    (Upright::Vertical, "Vertical"),
    (Upright::Full, "Full"),
];

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
        "Tone Curve",
        &[
            Curve,
            Slider("curve.highlights"),
            Slider("curve.lights"),
            Slider("curve.darks"),
            Slider("curve.shadows"),
            Slider("curve.refineSaturation"),
        ],
    ),
    (
        "HSL / Color",
        &[
            HslTabs,
            HslSub(Hsl::Hue),
            Named("mixer.red.hue", "Red"),
            Named("mixer.orange.hue", "Orange"),
            Named("mixer.yellow.hue", "Yellow"),
            Named("mixer.green.hue", "Green"),
            Named("mixer.aqua.hue", "Aqua"),
            Named("mixer.blue.hue", "Blue"),
            Named("mixer.purple.hue", "Purple"),
            Named("mixer.magenta.hue", "Magenta"),
            HslSub(Hsl::Saturation),
            Named("mixer.red.sat", "Red"),
            Named("mixer.orange.sat", "Orange"),
            Named("mixer.yellow.sat", "Yellow"),
            Named("mixer.green.sat", "Green"),
            Named("mixer.aqua.sat", "Aqua"),
            Named("mixer.blue.sat", "Blue"),
            Named("mixer.purple.sat", "Purple"),
            Named("mixer.magenta.sat", "Magenta"),
            HslSub(Hsl::Luminance),
            Named("mixer.red.lum", "Red"),
            Named("mixer.orange.lum", "Orange"),
            Named("mixer.yellow.lum", "Yellow"),
            Named("mixer.green.lum", "Green"),
            Named("mixer.aqua.lum", "Aqua"),
            Named("mixer.blue.lum", "Blue"),
            Named("mixer.purple.lum", "Purple"),
            Named("mixer.magenta.lum", "Magenta"),
        ],
    ),
    // Each wheel's Hue, Saturation and Luminance, in the order a single
    // wheel's view shows them; 3-Way shows the three luminances alone.
    (
        "Color Grading",
        &[
            Wheels,
            Slider("grading.shadows.hue"),
            Slider("grading.shadows.sat"),
            Slider("grading.shadows.lum"),
            Slider("grading.midtones.hue"),
            Slider("grading.midtones.sat"),
            Slider("grading.midtones.lum"),
            Slider("grading.highlights.hue"),
            Slider("grading.highlights.sat"),
            Slider("grading.highlights.lum"),
            Slider("grading.global.hue"),
            Slider("grading.global.sat"),
            Slider("grading.global.lum"),
            Slider("grading.blending"),
            Slider("grading.balance"),
        ],
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
    (
        "Transform",
        &[
            UprightModes,
            ConstrainCrop,
            Sub("Transform"),
            Slider("geometry.vertical"),
            Slider("geometry.horizontal"),
            Slider("geometry.rotate"),
            Slider("geometry.aspect"),
            Slider("geometry.scale"),
            Named("geometry.offsetX", "X Offset"),
            Named("geometry.offsetY", "Y Offset"),
        ],
    ),
];

impl Row {
    /// The row's control as it's shown, if it's a slider.
    fn spec(&self) -> Option<ControlSpec> {
        match self {
            Slider(id) => controls::find(id).copied(),
            Named(id, label) => controls::find(id).map(|spec| ControlSpec { label, ..*spec }),
            Sub(_) | RemoveCa | ProfileCorrections | UprightModes | ConstrainCrop | Curve | HslTabs | HslSub(_) | Wheels => None,
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
        "Tone Curve" => &[Group::ToneCurve],
        "HSL / Color" => &[Group::Color],
        "Color Grading" => &[Group::ColorGrading],
        "Detail" => &[Group::Sharpening, Group::NoiseReduction],
        "Lens Corrections" => &[Group::LensCorrections],
        "Transform" => &[Group::Transform],
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
#[derive(Clone, Debug, Default, PartialEq)]
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
    /// An Upright button was clicked.
    pub upright: Option<Upright>,
    /// Constrain Crop was ticked, or unticked.
    pub constrain_crop: Option<bool>,
    /// A panel's switch was flipped, or one of its sliders touched while
    /// it was off: its groups, and whether they're on now.
    pub switch: Option<(&'static [Group], bool)>,
    /// Something done in the tone curve's graph.
    pub curve: Option<curve::Out>,
    /// Something done to a grading wheel.
    pub wheel: Option<(Which, wheel::Out)>,
}

/// What the panel shows: a frame's settings and what it has to go with
/// them.
pub struct Shown<'a> {
    pub settings: &'a DevelopSettings,
    /// What a double-click resets each slider to.
    pub fresh: &'a DevelopSettings,
    /// The slider the keys act on.
    pub selected: usize,
    /// What the frame's camera did, which names its profile.
    pub camera: &'a Camera,
    /// The groups switched off.
    pub off: &'a [Group],
    /// The frame's, behind the tone curve.
    pub histogram: Option<&'a Histogram>,
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

/// The panels for what's `shown`, each in its view.
pub fn show(ui: &mut Ui, theme: &Theme, shown_: &Shown, views: &mut Views) -> PanelOut {
    let Shown { settings, fresh, selected, camera, off, histogram } = *shown_;
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
        let before = out.clone();
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
                UprightModes => {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for (mode, label) in UPRIGHTS {
                            let button = egui::Button::new(egui::RichText::new(label).size(12.0)).selected(settings.geometry.upright == mode);
                            let hover = if mode == Upright::Guided { tip(Command::GuidedUpright) } else { format!("Upright: {label}") };
                            if ui.add(button).on_hover_text(hover).clicked() {
                                out.upright = Some(mode);
                            }
                        }
                    });
                    // Upright reads the frame through its lens corrections, as Lightroom says.
                    if settings.geometry.upright != Upright::Off && !settings.optics.lens_profile {
                        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover());
                        let text = "Works best with Enable Profile Corrections";
                        ui.painter().text(row.left_center() + vec2(4.0, 1.0), Align2::LEFT_CENTER, text, FontId::proportional(11.5), theme.muted);
                    }
                }
                ConstrainCrop => {
                    let mut ticked = settings.geometry.constrain_crop;
                    if ui.checkbox(&mut ticked, "Constrain Crop").changed() {
                        out.constrain_crop = Some(ticked);
                    }
                }
                Curve => {
                    let curved = curve::show(ui, theme, &settings.curve, &mut views.curve, histogram);
                    if curved != curve::Out::default() {
                        out.curve = Some(curved);
                    }
                }
                HslTabs => tabs(ui, theme, &mut views.hsl, &Hsl::TABS, Hsl::label),
                HslSub(attribute) => {
                    if views.hsl == Hsl::All {
                        sub_heading(ui, theme, attribute.label());
                    }
                }
                Wheels => {
                    tabs(ui, theme, &mut views.grading, &Grading::VIEWS, Grading::label);
                    if let Some(turned) = wheels(ui, theme, &settings.grading, views.grading) {
                        out.wheel = Some(turned);
                    }
                }
                Slider(id) | Named(id, _) => {
                    let Some(spec) = row.spec() else { continue };
                    if !shown(id, views) {
                        index += 1;
                        continue;
                    }
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

/// A row of tabs, the one shown underlined: Lightroom's words over HSL and
/// Color Grading.
fn tabs<T: Copy + PartialEq>(ui: &mut Ui, theme: &Theme, view: &mut T, choices: &[T], label: fn(T) -> &'static str) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        ui.add_space(4.0);
        for &choice in choices {
            let on = *view == choice;
            let text = egui::RichText::new(label(choice)).size(12.0).color(if on { theme.foreground } else { theme.muted });
            let response = ui.add(egui::Label::new(text).sense(Sense::click()));
            if on {
                let under = response.rect.left_bottom() + vec2(0.0, 1.0);
                ui.painter().line_segment([under, under + vec2(response.rect.width(), 0.0)], egui::Stroke::new(1.5, theme.accent));
            }
            if response.clicked() {
                *view = choice;
            }
        }
    });
    ui.add_space(4.0);
}

/// Color Grading's wheels in its view: 3-Way's midtones over shadows and
/// highlights, as Lightroom lays them out, or one large. The wheel
/// turned, if one was.
fn wheels(ui: &mut Ui, theme: &Theme, grading: &lightcraft_develop::ColorGrading, view: Grading) -> Option<(Which, wheel::Out)> {
    let width = ui.available_width();
    let mut turned = None;
    let mut one = |ui: &mut Ui, which: Which, centre: egui::Pos2, rad: f32, readout: bool| {
        let out = wheel::show(ui, theme, which, which.of(grading), centre, rad, readout);
        if out != wheel::Out::default() {
            turned = Some((which, out));
        }
    };
    match view {
        Grading::ThreeWay => {
            let rad = (width * 0.15).clamp(32.0, 46.0);
            let (area, _) = ui.allocate_exact_size(vec2(width, rad * 4.0 + 64.0), Sense::hover());
            let top = area.top() + rad + 18.0;
            let bottom = top + rad * 2.0 + 26.0;
            one(ui, Which::Midtones, egui::pos2(area.center().x, top), rad, true);
            one(ui, Which::Shadows, egui::pos2(area.left() + width * 0.27, bottom), rad, true);
            one(ui, Which::Highlights, egui::pos2(area.left() + width * 0.73, bottom), rad, true);
        }
        Grading::One(which) => {
            let rad = (width * 0.3).clamp(40.0, 80.0);
            let (area, _) = ui.allocate_exact_size(vec2(width, rad * 2.0 + 30.0), Sense::hover());
            one(ui, which, egui::pos2(area.center().x, area.top() + rad + 4.0), rad, false);
        }
    }
    turned
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
        assert_eq!(of("curve.refineSaturation"), [Group::ToneCurve]);
        assert_eq!(of("mixer.magenta.lum"), [Group::Color]);
        assert_eq!(of("grading.balance"), [Group::ColorGrading]);
        assert_eq!(of("detail.nrColor"), [Group::Sharpening, Group::NoiseReduction]);
        assert_eq!(of("optics.defringeGreenHueHi"), [Group::LensCorrections]);
        assert_eq!(of("geometry.offsetY"), [Group::Transform]);
        assert!(switches_of(sliders().count()).is_empty());
    }

    #[test]
    fn each_view_shows_its_own_sliders() {
        let ids = |views: &Views, prefix: &str| -> Vec<&'static str> {
            sliders().enumerate().filter(|(i, spec)| spec.id.starts_with(prefix) && visible(*i, views)).map(|(_, spec)| spec.id).collect()
        };
        let mut views = Views::default();
        assert_eq!(ids(&views, "curve."), ["curve.highlights", "curve.lights", "curve.darks", "curve.shadows"]);
        assert_eq!(ids(&views, "mixer.").len(), 8);
        assert!(ids(&views, "mixer.").iter().all(|id| id.ends_with(".hue")));
        assert_eq!(ids(&views, "grading."), ["grading.shadows.lum", "grading.midtones.lum", "grading.highlights.lum", "grading.blending", "grading.balance"]);
        views.curve = Channel::Blue;
        assert_eq!(ids(&views, "curve."), ["curve.refineSaturation"]);
        views.hsl = Hsl::All;
        assert_eq!(ids(&views, "mixer.").len(), 24);
        views.hsl = Hsl::Luminance;
        assert!(ids(&views, "mixer.").iter().all(|id| id.ends_with(".lum")));
        views.grading = Grading::One(Which::Global);
        assert_eq!(ids(&views, "grading."), ["grading.global.hue", "grading.global.sat", "grading.global.lum", "grading.blending", "grading.balance"]);
    }

    #[test]
    fn stepping_passes_over_hidden_sliders_and_stops_at_the_ends() {
        let views = Views::default();
        let at = |id: &str| sliders().position(|spec| spec.id == id).unwrap();
        assert_eq!(step(at("curve.shadows"), 1, &views), at("mixer.red.hue"));
        assert_eq!(step(at("mixer.red.hue"), -1, &views), at("curve.shadows"));
        // From a hidden one: the next shown either way.
        assert_eq!(step(at("curve.refineSaturation"), 1, &views), at("mixer.red.hue"));
        assert_eq!(step(at("curve.refineSaturation"), -1, &views), at("curve.shadows"));
        assert_eq!(step(0, -1, &views), 0);
        let last = sliders().count() - 1;
        assert_eq!(step(last, 1, &views), last);
    }

    #[test]
    fn the_sliders_are_lightrooms_in_lightrooms_order() {
        let labels: Vec<&str> = sliders().map(|spec| spec.label).collect();
        assert_eq!(labels, [
            "Temp", "Tint",
            "Exposure", "Contrast", "Highlights", "Shadows", "Whites", "Blacks",
            "Amount", "Highlights", "Shadows",
            "Texture", "Clarity", "Dehaze", "Vibrance", "Saturation",
            "Highlights", "Lights", "Darks", "Shadows", "Refine Saturation",
            "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple", "Magenta",
            "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple", "Magenta",
            "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple", "Magenta",
            "Shadows Hue", "Shadows Saturation", "Shadows Luminance",
            "Midtones Hue", "Midtones Saturation", "Midtones Luminance",
            "Highlights Hue", "Highlights Saturation", "Highlights Luminance",
            "Global Hue", "Global Saturation", "Global Luminance",
            "Blending", "Balance",
            "Amount", "Radius", "Detail", "Masking", "Luminance", "Color",
            "Distortion", "Vignetting",
            "Purple Amount", "Purple Hue Low", "Purple Hue High", "Green Amount", "Green Hue Low", "Green Hue High",
            "Vertical", "Horizontal", "Rotate", "Aspect", "Scale", "X Offset", "Y Offset",
        ]);
    }
}
