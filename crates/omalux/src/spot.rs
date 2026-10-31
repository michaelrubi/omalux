//! Spot Removal (Q), for sensor dust: Lightroom's tool cut down to heal
//! circles. A click puts one down, a click on one takes hold of it, a drag
//! moves it; `[` and `]` make it (or the next) smaller and larger, Delete
//! takes it (or else the last) away, A is Visualize Spots, Return puts the
//! tool away and Esc puts it away with the spots as they were. Its panel,
//! under the histogram while it's open, has the held spot's Size, Feather
//! and Opacity, Visualize Spots' threshold, and Find Dust.
//!
//! Spots are kept where they are on the frame before its lens corrections,
//! crop and transform (see `omalux_engine::dust`), so the tool shows the
//! frame that way while it's open.

use std::path::{Path, PathBuf};

use egui::{Color32, CursorIcon, Pos2, Rect, Sense, Stroke, TextureHandle, Ui, pos2, vec2};
use lightcraft_develop::controls::{self, ControlSpec, Track};
use lightcraft_develop::{DevelopSettings, Spot};
use omalux_engine::crop::Point;
use omalux_engine::dust;

use crate::develop;
use crate::slider::{SliderOut, slider};
use crate::theme::Theme;

/// How much `[` and `]` change a spot's size.
pub const STEP: f64 = 1.15;
/// Visualize Spots' threshold to begin with.
const THRESHOLD: u8 = 50;
/// Find Dust's sensitivity: Lightcraft's detector's middle.
pub const SENSITIVITY: f32 = 50.0;

/// What happened on the frame this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Out {
    /// A click where there's no spot: a new one, here.
    pub added: Option<Point>,
    /// A spot dragged here.
    pub moved: Option<(usize, Point)>,
    /// A drag ended: what it changed is one step.
    pub stopped: bool,
}

/// What was done in the tool's panel this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PanelOut {
    /// The held spot's Size (a share of the long edge), Feather and
    /// Opacity.
    pub size: SliderOut,
    pub feather: SliderOut,
    pub opacity: SliderOut,
    pub find: bool,
    pub clear: bool,
    pub done: bool,
}

/// The tool, while it's open.
pub struct Tool {
    /// The frame it's on, and that frame's spots when the tool came to it:
    /// what Esc goes back to.
    was: Option<(PathBuf, Vec<Spot>)>,
    /// The spot held, by its place in the frame's spots.
    pub held: Option<usize>,
    /// The spot being dragged.
    moving: Option<usize>,
    /// How large the next spot is.
    pub size: f64,
    /// Visualize Spots' threshold, while it's on.
    pub visualize: Option<u8>,
}

impl Default for Tool {
    fn default() -> Self {
        Self { was: None, held: None, moving: None, size: dust::SIZE, visualize: None }
    }
}

/// The largest rectangle of `size`'s shape centred in `area`.
fn fit(size: egui::Vec2, area: Rect) -> Rect {
    let scale = (area.width() / size.x).min(area.height() / size.y);
    Rect::from_center_size(area.center(), size * scale)
}

impl Tool {
    /// The tool is on `path`, whose settings are `settings`: remembered
    /// the first time, for [`Self::was`].
    pub fn on(&mut self, path: &Path, settings: &DevelopSettings) {
        if self.was.as_ref().is_none_or(|(on, _)| on != path) {
            self.was = Some((path.to_path_buf(), settings.spots.clone()));
            (self.held, self.moving) = (None, None);
        }
    }

    /// The spots `path` had when the tool came to it.
    pub fn was(&self, path: &Path) -> Option<Vec<Spot>> {
        self.was.as_ref().filter(|(on, _)| on == path).map(|(_, spots)| spots.clone())
    }

    /// Visualize Spots on, at its threshold, or off.
    pub fn toggle_visualize(&mut self) {
        self.visualize = if self.visualize.is_some() { None } else { Some(THRESHOLD) };
    }

    /// Draw the frame, `dims` in size as it's shown, in `area`, with
    /// `spots` over it, and take the mouse. `whole` is its render without
    /// its lens corrections, crop or transform, once there is one.
    pub fn show(&mut self, ui: &mut Ui, theme: &Theme, area: Rect, whole: Option<&TextureHandle>, dims: (f64, f64), spots: &[Spot]) -> Out {
        let mut out = Out::default();
        let frame = fit(vec2(dims.0 as f32, dims.1 as f32), area);
        if !frame.is_positive() {
            return out;
        }
        let painter = ui.painter_at(area);
        if let Some(texture) = whole {
            painter.image(texture.id(), frame, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        let long = frame.width().max(frame.height());
        let on_screen = |p: Point| frame.min + frame.size() * vec2(p.x as f32, p.y as f32);
        let in_frame = |at: Pos2| {
            let at = frame.clamp(at);
            Point::new(f64::from((at.x - frame.left()) / frame.width()), f64::from((at.y - frame.top()) / frame.height()))
        };
        #[cfg(test)]
        ui.data_mut(|d| d.insert_temp(egui::Id::new("spot frame"), frame));
        let under = |at: Pos2| {
            spots.iter().enumerate().rev().find_map(|(i, spot)| {
                let centre = on_screen(*spot.points.first()?);
                (centre.distance(at) <= spot.size as f32 * long + 4.0).then_some(i)
            })
        };

        let response = ui.interact(area, ui.id().with("spot removal"), Sense::click_and_drag());
        if response.clicked()
            && let Some(at) = response.interact_pointer_pos()
        {
            match under(at) {
                Some(i) => self.held = Some(i),
                None if frame.contains(at) => {
                    out.added = Some(in_frame(at));
                    self.held = Some(spots.len());
                }
                None => self.held = None,
            }
        }
        if response.drag_started() {
            self.moving = ui.input(|i| i.pointer.press_origin()).and_then(under);
            self.held = self.moving.or(self.held);
        }
        if let (Some(i), Some(at)) = (self.moving, response.interact_pointer_pos())
            && response.dragged()
        {
            out.moved = Some((i, in_frame(at)));
        }
        if response.drag_stopped() {
            out.stopped = self.moving.take().is_some();
        }

        for (i, spot) in spots.iter().enumerate() {
            let Some(at) = spot.points.first() else { continue };
            let (centre, radius) = (on_screen(*at), spot.size as f32 * long);
            let colour = if self.held == Some(i) { theme.accent } else { Color32::WHITE };
            painter.circle_stroke(centre, radius, Stroke::new(3.0, Color32::from_black_alpha(140)));
            painter.circle_stroke(centre, radius, Stroke::new(1.5, colour));
        }
        if let Some(at) = response.hover_pos().filter(|_| response.hovered()) {
            if under(at).is_some() {
                ui.ctx().set_cursor_icon(CursorIcon::Grab);
            } else if frame.contains(at) {
                // The next spot, where a click would put it.
                painter.circle_stroke(at, self.size as f32 * long, Stroke::new(1.0, Color32::from_white_alpha(160)));
                ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
            }
        }
        out
    }
}

/// A slider of the tool's, under `label`, from `min` to `max`.
fn spec(id: &'static str, label: &'static str, (min, max, default): (f64, f64, f64), decimals: u8) -> Option<ControlSpec> {
    let like = controls::find("crop.angle").copied()?;
    Some(ControlSpec { id, label, min, max, default, step: if decimals == 0 { 1.0 } else { 0.1 }, decimals, track: Track::Plain, ..like })
}

/// The tool's panel: the held spot's Size, Feather and Opacity (Size alone,
/// for the next spot, with none held), Visualize Spots and its threshold,
/// Find Dust, Clear and Close.
pub fn panel(ui: &mut Ui, theme: &Theme, tool: &mut Tool, spots: &[Spot]) -> PanelOut {
    let mut out = PanelOut::default();
    develop::heading(ui, theme, "Spot Removal");
    let held = tool.held.and_then(|i| spots.get(i));
    // Size is shown as a percentage of the long edge.
    if let Some(spec) = spec("spot.size", "Size", (dust::SIZES.0 * 100.0, dust::SIZES.1 * 100.0, dust::SIZE * 100.0), 1) {
        let size = held.map_or(tool.size, |spot| spot.size);
        out.size = slider(ui, theme, &spec, size * 100.0, dust::SIZE * 100.0, false);
        out.size.value = out.size.value.map(|v| v / 100.0);
    }
    if let Some(spot) = held {
        if let Some(spec) = spec("spot.feather", "Feather", (0.0, 100.0, 50.0), 0) {
            out.feather = slider(ui, theme, &spec, spot.feather, 50.0, false);
        }
        if let Some(spec) = spec("spot.opacity", "Opacity", (0.0, 100.0, 100.0), 0) {
            out.opacity = slider(ui, theme, &spec, spot.opacity, 100.0, false);
        }
    }
    ui.add_space(4.0);
    let mut visualize = tool.visualize.is_some();
    if ui.checkbox(&mut visualize, "Visualize Spots").on_hover_text("A").changed() {
        tool.toggle_visualize();
    }
    if let (Some(threshold), Some(spec)) = (tool.visualize, spec("spot.threshold", "Threshold", (0.0, 100.0, f64::from(THRESHOLD)), 0)) {
        let slid = slider(ui, theme, &spec, f64::from(threshold), f64::from(THRESHOLD), false);
        if let Some(value) = slid.value {
            tool.visualize = Some(value.round().clamp(0.0, 100.0) as u8);
        }
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        out.find = ui.button("Find Dust").on_hover_text("Puts a heal circle on each speck of dust found").clicked();
        out.clear = ui.button("Clear").on_hover_text("Takes every spot away").clicked();
        out.done = ui.button("Close").on_hover_text("Return, or Q").clicked();
    });
    ui.add_space(8.0);
    out
}
