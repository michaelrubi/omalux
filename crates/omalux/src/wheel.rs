//! Color Grading's wheels, as Lightroom's: drag the puck for hue and
//! saturation (Shift keeps the hue, Ctrl the saturation, Alt moves it a
//! tenth as far), double-click to reset.
//!
//! Lifted from Lightcraft's `grading_wheel`, `WheelGrab`, `wheel_pick`
//! and `wheel_mesh` (`ui-egui/src/panels/edit.rs` at `0da998f2`, MIT OR
//! Apache-2.0), restyled to Omarchy's theme, with the change handed back
//! rather than run, and without its eye (a look, which the panel's switch
//! is here).

use egui::{Align2, Color32, FontId, Mesh, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use egui::epaint::Vertex;
use lightcraft_develop::{ColorGrading, Wheel};

use crate::theme::Theme;

/// Which wheel: Lightroom's three, and Global.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    Shadows,
    Midtones,
    Highlights,
    Global,
}

impl Which {
    pub fn label(self) -> &'static str {
        match self {
            Which::Shadows => "Shadows",
            Which::Midtones => "Midtones",
            Which::Highlights => "Highlights",
            Which::Global => "Global",
        }
    }

    pub fn of(self, grading: &ColorGrading) -> Wheel {
        match self {
            Which::Shadows => grading.shadows,
            Which::Midtones => grading.midtones,
            Which::Highlights => grading.highlights,
            Which::Global => grading.global,
        }
    }

    /// Set its hue and saturation; its luminance stays.
    pub fn set(self, grading: &mut ColorGrading, hue: f64, sat: f64) {
        let wheel = match self {
            Which::Shadows => &mut grading.shadows,
            Which::Midtones => &mut grading.midtones,
            Which::Highlights => &mut grading.highlights,
            Which::Global => &mut grading.global,
        };
        (wheel.hue, wheel.sat) = (hue.rem_euclid(360.0).round(), sat.clamp(0.0, 100.0).round());
    }
}

/// What was done to a wheel this frame: its new hue and saturation, and
/// whether a drag began or ended (a drag's changes are one step to undo).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Out {
    pub change: Option<(f64, f64)>,
    pub drag_started: bool,
    pub drag_stopped: bool,
}

/// A wheel's modifiers: Shift keeps the hue, Ctrl the saturation (Shift
/// wins when both are down), Alt fine-adjusts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Mods {
    keep_hue: bool,
    keep_sat: bool,
    fine: bool,
}

impl Mods {
    fn from(m: egui::Modifiers) -> Self {
        Self { keep_hue: m.shift, keep_sat: m.command && !m.shift, fine: m.alt }
    }
}

/// What a constraint holds still while the pointer moves.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Lock {
    /// This hue (degrees): only the saturation changes, along its spoke.
    Hue(f32),
    /// This saturation (0 to 100): only the hue changes, round the circle.
    Sat(f32),
}

/// Pointer travel to puck travel with Alt held.
const FINE: f32 = 0.1;

/// The puck as it was when grabbed (or a modifier last changed): the hue
/// and saturation a constraint keeps, and where a fine drag measures
/// from.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Grab {
    mods: Mods,
    hue: f32,
    sat: f32,
    origin: Vec2,
}

impl Grab {
    /// A puck at the centre has no hue yet: the pointer's angle is kept.
    fn new(mods: Mods, hue: f32, sat: f32, origin: Vec2, rad: f32) -> Self {
        let sat = if sat.is_finite() { sat.clamp(0.0, 100.0) } else { 0.0 };
        let hue = if hue.is_finite() && sat >= 1.0 { hue.rem_euclid(360.0) } else { pick(origin, rad, None).0 };
        Self { mods, hue, sat, origin }
    }

    fn lock(&self) -> Option<Lock> {
        if self.mods.keep_hue {
            Some(Lock::Hue(self.hue))
        } else if self.mods.keep_sat {
            Some(Lock::Sat(self.sat))
        } else {
            None
        }
    }

    fn pick(&self, v: Vec2, rad: f32) -> (f32, f32) {
        let v = if self.mods.fine {
            let a = self.hue.to_radians();
            vec2(a.cos(), -a.sin()) * rad.max(1.0) * (self.sat / 100.0) + (v - self.origin) * FINE
        } else {
            v
        };
        pick(v, rad, self.lock())
    }
}

/// Hue (degrees) and saturation (0 to 100) for the pointer at `v` from a
/// wheel's centre, under a lock if one's held.
fn pick(v: Vec2, rad: f32, lock: Option<Lock>) -> (f32, f32) {
    let rad = rad.max(1.0);
    let v = if v.x.is_finite() && v.y.is_finite() { v } else { Vec2::ZERO };
    let angle = || ((-v.y).atan2(v.x).to_degrees() + 360.0) % 360.0;
    match lock {
        Some(Lock::Hue(h)) => {
            let a = h.to_radians();
            (h, ((v.x * a.cos() - v.y * a.sin()) / rad * 100.0).clamp(0.0, 100.0))
        }
        Some(Lock::Sat(s)) => (angle(), s),
        None => (angle(), (v.length() / rad * 100.0).min(100.0)),
    }
}

/// One wheel centred on `centre`, its name under it; the hue and
/// saturation over it while it's hovered or dragged if `readout`.
pub fn show(ui: &mut Ui, theme: &Theme, which: Which, wheel: Wheel, centre: Pos2, rad: f32, readout: bool) -> Out {
    let mut out = Out::default();
    let rect = Rect::from_center_size(centre, vec2(rad * 2.0, rad * 2.0));
    let id = ui.id().with(("wheel", which.label()));
    let response = ui
        .interact(rect, id, Sense::click_and_drag())
        .on_hover_text("Drag for hue and saturation: Shift keeps the hue, Ctrl the saturation, Alt moves finely. Double-click to reset.");
    // Where it is, for the headless tests.
    ui.data_mut(|d| d.insert_temp(egui::Id::new(("wheel", which.label())), rect));
    let painter = ui.painter();
    painter.add(mesh(centre, rad));
    painter.circle_stroke(centre, rad, Stroke::new(1.0, theme.darker_background));
    // The large single wheel: rings at 25, 50 and 75% saturation.
    if !readout {
        for f in [0.25, 0.5, 0.75] {
            painter.circle_stroke(centre, rad * f, Stroke::new(0.8, Color32::from_white_alpha(40)));
        }
    }
    let grab_id = id.with("grab");
    let mods = ui.input(|i| Mods::from(i.modifiers));
    let grab = match response.interact_pointer_pos() {
        Some(q) if response.dragged() || response.clicked() => {
            let held = ui.data(|d| d.get_temp::<Grab>(grab_id)).filter(|g| g.mods == mods && !response.drag_started());
            Some(held.unwrap_or_else(|| {
                // A fine drag counts from the press, not from where egui
                // first called it a drag.
                let origin = if response.drag_started() { ui.input(|i| i.pointer.press_origin()).unwrap_or(q) } else { q };
                let grab = Grab::new(mods, wheel.hue as f32, wheel.sat as f32, origin - centre, rad);
                ui.data_mut(|d| d.insert_temp(grab_id, grab));
                grab
            }))
        }
        _ => {
            ui.data_mut(|d| d.remove::<Grab>(grab_id));
            None
        }
    };
    // While a constraint's held, the one move left: the spoke, or the circle.
    let guide = Stroke::new(1.5, Color32::from_white_alpha(170));
    match grab.and_then(|g| g.lock()) {
        Some(Lock::Hue(h)) => {
            let a = h.to_radians();
            painter.line_segment([centre, centre + vec2(a.cos(), -a.sin()) * rad], guide);
        }
        Some(Lock::Sat(s)) => _ = painter.circle_stroke(centre, rad * s / 100.0, guide),
        None => {}
    }
    let a = (wheel.hue as f32).to_radians();
    let puck = centre + vec2(a.cos(), -a.sin()) * rad * (wheel.sat as f32 / 100.0);
    painter.circle_filled(centre, 2.0, Color32::WHITE);
    painter.circle_filled(puck, 5.0, Color32::WHITE);
    painter.circle_stroke(puck, 5.0, Stroke::new(1.0, Color32::BLACK));
    painter.text(pos2(centre.x, rect.bottom() + 10.0), Align2::CENTER_CENTER, which.label(), FontId::proportional(11.5), theme.muted);
    if readout && (response.hovered() || response.dragged()) {
        let text = format!("H {:.0}°  S {:.0}", wheel.hue, wheel.sat);
        painter.text(pos2(centre.x, rect.top() - 9.0), Align2::CENTER_CENTER, text, FontId::proportional(11.0), theme.muted);
    }
    out.drag_started = response.drag_started();
    out.drag_stopped = response.drag_stopped() || response.clicked();
    if (response.dragged() || response.clicked())
        && let Some(q) = response.interact_pointer_pos()
    {
        let (hue, sat) = grab.map_or_else(|| pick(q - centre, rad, None), |g| g.pick(q - centre, rad));
        out.change = Some((f64::from(hue), f64::from(sat)));
    }
    if response.double_clicked() {
        out.change = Some((0.0, 0.0));
        out.drag_stopped = true;
    }
    out
}

/// A hue disc whose rim is fully saturated at full brightness, fading to
/// grey at the centre, in rings so the saturation ramps evenly outwards.
fn mesh(centre: Pos2, rad: f32) -> Mesh {
    const SEGMENTS: u32 = 72;
    const RINGS: u32 = 12;
    const CENTRE_V: f32 = 0.6;
    let colour = |hue: f32, t: f32| {
        let rgb = lightcraft_color::perceptual::hsv_to_rgb(hue, t, CENTRE_V + (1.0 - CENTRE_V) * t);
        let u = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
        Color32::from_rgb(u(rgb[0]), u(rgb[1]), u(rgb[2]))
    };
    let mut mesh = Mesh::default();
    mesh.vertices.push(Vertex { pos: centre, uv: Pos2::ZERO, color: colour(0.0, 0.0) });
    let row = SEGMENTS + 1;
    for k in 1..=RINGS {
        let t = k as f32 / RINGS as f32;
        for i in 0..=SEGMENTS {
            let a = i as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            mesh.vertices.push(Vertex { pos: centre + vec2(a.cos(), -a.sin()) * rad * t, uv: Pos2::ZERO, color: colour(a.to_degrees(), t) });
        }
    }
    for i in 0..SEGMENTS {
        mesh.indices.extend([0, 1 + i, 2 + i]);
    }
    for k in 1..RINGS {
        let inner = 1 + (k - 1) * row;
        let outer = inner + row;
        for i in 0..SEGMENTS {
            mesh.indices.extend([inner + i, outer + i, outer + i + 1, inner + i, outer + i + 1, inner + i + 1]);
        }
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pointer_picks_the_hue_by_angle_and_the_saturation_by_distance() {
        let (hue, sat) = pick(vec2(50.0, 0.0), 100.0, None);
        assert_eq!((hue.round(), sat.round()), (0.0, 50.0));
        let (hue, sat) = pick(vec2(0.0, -200.0), 100.0, None);
        assert_eq!((hue.round(), sat.round()), (90.0, 100.0), "up is 90°, and outside the rim is full");
        assert_eq!(pick(vec2(f32::NAN, 1.0), 100.0, None), (0.0, 0.0));
    }

    #[test]
    fn shift_keeps_the_hue_ctrl_the_saturation_and_alt_moves_finely() {
        let shift = Mods { keep_hue: true, ..Mods::default() };
        let grab = Grab::new(shift, 120.0, 40.0, vec2(0.0, 0.0), 100.0);
        assert_eq!(grab.pick(vec2(80.0, 0.0), 100.0).0, 120.0);
        let ctrl = Mods { keep_sat: true, ..Mods::default() };
        let grab = Grab::new(ctrl, 120.0, 40.0, vec2(0.0, 0.0), 100.0);
        assert_eq!(grab.pick(vec2(0.0, -90.0), 100.0), (90.0, 40.0));
        let alt = Mods { fine: true, ..Mods::default() };
        let grab = Grab::new(alt, 0.0, 50.0, vec2(50.0, 0.0), 100.0);
        let (_, sat) = grab.pick(vec2(150.0, 0.0), 100.0);
        assert!((sat - 60.0).abs() < 0.01, "a tenth of the pointer's travel: {sat}");
    }

    #[test]
    fn setting_a_wheel_keeps_its_luminance_and_wraps_its_hue() {
        let mut grading = ColorGrading::default();
        grading.highlights.lum = 12.0;
        Which::Highlights.set(&mut grading, 370.4, 140.0);
        assert_eq!((grading.highlights.hue, grading.highlights.sat, grading.highlights.lum), (10.0, 100.0, 12.0));
        assert_eq!(Which::Shadows.of(&grading), Wheel::default());
    }
}
