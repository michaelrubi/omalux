//! The loupe: the current frame, as large as fits, or at 100%. What it
//! shows of each frame the renderer was asked for: the camera's embedded
//! JPEG at once, and the render once it's in.
//!
//! At 100% (Z, or a click: a tap toggles, a hold is a look that ends when
//! it's let go; a drag or the wheel pans) the frame is drawn at one of its
//! pixels to one of the screen's. What's on screen is rendered from the
//! raw at full size; until that's in, the render that fits is shown
//! enlarged. The zoom's behaviour is Omacull's (`omacull/src/loupe.rs`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{Pos2, Rect, Response, TextureHandle, TextureOptions, Ui, Vec2, vec2};
use lightcraft_develop::DevelopSettings;
use lightcraft_pipeline::SourceInfo;
use lightcraft_raster::Histogram;
use omalux_engine::crop::Orientation;
use omalux_engine::look::Camera;

use crate::render::{Done, Key, ZoomKey};

/// The pasteboard left round the frame, in points.
const MARGIN: f32 = 12.0;
/// Held longer than this (seconds), the zoom key or a click is a look that
/// ends when it's let go; shorter, it toggles.
pub const HOLD: f64 = 0.3;

/// A press of the zoom key or the mouse that may become a hold.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Hold {
    /// egui's time when it started.
    since: f64,
    /// Whether it zoomed in.
    from_fit: bool,
    dragged: bool,
    /// The key, not the mouse.
    key: bool,
}

/// The window of a frame that was on screen at 100%, rendered.
pub struct Zoomed {
    pub texture: TextureHandle,
    pub key: ZoomKey,
}

/// A frame's render on screen.
pub struct Render {
    pub texture: TextureHandle,
    pub histogram: Histogram,
    /// Which render it is: only the tests ask, so far.
    #[cfg_attr(not(test), allow(dead_code))]
    pub key: Key,
    /// The settings it was rendered with.
    pub settings: Arc<DevelopSettings>,
}

/// What the mouse does on the loupe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mouse {
    /// A click zooms to 100% and back, a drag pans.
    Zoom,
    /// The White Balance Selector: a click picks.
    Pick,
    /// A tool has it, and draws the frame itself (Crop & Straighten, on
    /// [`Loupe::whole`]).
    Tool,
}

/// What the renderer finished that isn't a picture.
pub enum News {
    /// A frame's raw is decoded.
    Decoded(PathBuf, Arc<SourceInfo>, Arc<Camera>),
    /// A frame couldn't be developed, and why.
    Failed(PathBuf, String),
}

/// What there is to show of a frame.
#[derive(Default)]
pub struct Shown {
    /// The camera's JPEG.
    pub preview: Option<TextureHandle>,
    pub render: Option<Render>,
    /// How large the frame is at 100%, in pixels.
    pub full: Option<(usize, usize)>,
    pub zoomed: Option<Zoomed>,
}

pub struct Loupe {
    shown: HashMap<PathBuf, Shown>,
    /// The room the frame had when last drawn, in pixels: what renders
    /// are asked for.
    size: (usize, usize),
    /// At 100%, not fitted.
    zoomed: bool,
    /// The point in the middle of the loupe at 100%, as shares of the
    /// frame's width and height. Kept when stepping, so the same spot can
    /// be checked from frame to frame.
    centre: [f32; 2],
    hold: Option<Hold>,
    /// The room the frame has, where the whole frame is drawn in it, and
    /// its size at 100% in points: as last drawn.
    area: Rect,
    fit: Rect,
    full: Vec2,
}

impl Default for Loupe {
    fn default() -> Self {
        let (shown, hold, nowhere) = (HashMap::new(), None, Rect::NOTHING);
        Self { shown, size: (0, 0), zoomed: false, centre: [0.5, 0.5], hold, area: nowhere, fit: nowhere, full: Vec2::ZERO }
    }
}

/// The largest rectangle of `size`'s shape centred in `area`.
fn fit(size: Vec2, area: Rect) -> Rect {
    let scale = (area.width() / size.x).min(area.height() / size.y);
    Rect::from_center_size(area.center(), size * scale)
}

impl Loupe {
    pub fn size(&self) -> (usize, usize) {
        self.size
    }

    /// The point in the middle of the loupe while it's at 100%.
    pub fn zoom(&self) -> Option<[f32; 2]> {
        (self.zoomed && self.full.x > 0.0).then(|| self.clamped(self.centre))
    }

    /// `centre` moved as little as it takes for the frame at 100% to cover
    /// the loupe, or to sit in its middle where it's smaller.
    fn clamped(&self, [u, v]: [f32; 2]) -> [f32; 2] {
        let axis = |c: f32, view: f32, size: f32| {
            let half = view / 2.0 / size;
            if half >= 0.5 { 0.5 } else { c.clamp(half, 1.0 - half) }
        };
        [axis(u, self.area.width(), self.full.x), axis(v, self.area.height(), self.full.y)]
    }

    /// Zoom in, keeping the point under `at` where it is; elsewhere, where
    /// it was last zoomed.
    fn zoom_in(&mut self, at: Option<Pos2>) {
        if let Some(at) = at.filter(|p| self.area.contains(*p) && self.fit.is_positive()) {
            let u = ((at - self.fit.min) / self.fit.size()).clamp(Vec2::ZERO, Vec2::splat(1.0));
            let from_middle = (at - self.area.center()) / self.full;
            self.centre = [u.x - from_middle.x, u.y - from_middle.y];
        }
        self.centre = self.clamped(self.centre);
        self.zoomed = true;
    }

    /// The zoom key went down, with the pointer at `at`: zoom in, or out
    /// if zoomed. Key repeats while it's held do nothing.
    pub fn key_down(&mut self, at: Option<Pos2>, time: f64) {
        if self.hold.is_some_and(|h| h.key) || self.full.x <= 0.0 {
            return;
        }
        if self.zoomed {
            self.zoomed = false;
        } else {
            self.zoom_in(at);
            self.hold = Some(Hold { since: time, from_fit: true, dragged: false, key: true });
        }
    }

    /// The zoom key isn't down: if it was held, the look is over.
    pub fn key_up(&mut self, time: f64) {
        if let Some(hold) = self.hold.filter(|h| h.key) {
            self.hold = None;
            if time - hold.since >= HOLD {
                self.zoomed = false;
            }
        }
    }

    /// Move the frame by `delta` points.
    fn pan(&mut self, delta: Vec2) {
        self.centre = self.clamped([self.centre[0] - delta.x / self.full.x, self.centre[1] - delta.y / self.full.y]);
    }

    /// The mouse on the loupe: press to zoom in at the pointer, hold for a
    /// look, drag to pan, click again to zoom out; the wheel pans.
    fn pointer(&mut self, ui: &Ui, response: &Response) {
        let (pressed, released, time, scroll) = ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_released(), i.time, i.smooth_scroll_delta));
        if pressed && response.is_pointer_button_down_on() {
            let from_fit = !self.zoomed;
            if from_fit {
                self.zoom_in(response.interact_pointer_pos());
            }
            self.hold = Some(Hold { since: time, from_fit, dragged: false, key: false });
        }
        if self.zoomed && response.dragged() {
            self.pan(response.drag_delta());
            if let Some(hold) = &mut self.hold {
                hold.dragged = true;
            }
        }
        if released && let Some(hold) = self.hold.filter(|h| !h.key) {
            self.hold = None;
            // A quick click toggles; a hold, or a drag, from whole was a look.
            let quick = time - hold.since < HOLD && !hold.dragged;
            if hold.from_fit != quick {
                self.zoomed = false;
            }
        }
        if self.zoomed && response.hovered() && scroll != Vec2::ZERO {
            self.pan(scroll);
        }
    }

    /// Where the whole frame is when it's at 100%, on whole pixels so it's
    /// sharp.
    fn zoomed_rect(&self, pixels_per_point: f32) -> Rect {
        let [u, v] = self.clamped(self.centre);
        let min = self.area.center() - vec2(u * self.full.x, v * self.full.y);
        let min = (min.to_vec2() * pixels_per_point).round() / pixels_per_point;
        Rect::from_min_size(min.to_pos2(), self.full)
    }

    #[cfg(test)]
    pub fn shown(&self, path: &Path) -> Option<&Shown> {
        self.shown.get(path)
    }

    /// The histogram of a frame's render, once it has one.
    pub fn histogram(&self, path: &Path) -> Option<&Histogram> {
        Some(&self.shown.get(path)?.render.as_ref()?.histogram)
    }

    /// Forget every frame `keep` doesn't name.
    pub fn retain(&mut self, keep: impl Fn(&Path) -> bool) {
        self.shown.retain(|path, _| keep(path));
    }

    /// Take in what the renderer finished: the pictures are kept to show,
    /// the rest is returned.
    pub fn take(&mut self, ctx: &egui::Context, done: Vec<Done>) -> Vec<News> {
        let mut news = Vec::new();
        for done in done {
            match done {
                Done::Preview { path, image } => {
                    let texture = ctx.load_texture(format!("preview {}", path.display()), image, TextureOptions::LINEAR);
                    self.shown.entry(path).or_default().preview = Some(texture);
                }
                Done::Rendered { path, key, image, histogram, settings, ms } => {
                    log::debug!("{}: {}×{}{} in {ms:.0} ms", path.display(), image.size[0], image.size[1], if key.draft { " draft" } else { "" });
                    let texture = ctx.load_texture(format!("render {}", path.display()), image, TextureOptions::LINEAR);
                    self.shown.entry(path).or_default().render = Some(Render { texture, key, histogram, settings });
                }
                Done::Decoded { path, info, camera, full } => {
                    self.shown.entry(path.clone()).or_default().full = Some(full);
                    news.push(News::Decoded(path, info, camera));
                }
                Done::Zoomed { path, key, image } => {
                    let texture = ctx.load_texture(format!("100% {}", path.display()), image, TextureOptions::NEAREST);
                    let shown = self.shown.entry(path).or_default();
                    (shown.full, shown.zoomed) = (Some(key.full), Some(Zoomed { texture, key }));
                }
                Done::Failed { path, error } => news.push(News::Failed(path, error)),
            }
        }
        news
    }

    /// The room the frame has, as last drawn.
    pub fn area(&self) -> Rect {
        self.area
    }

    /// A frame's render, if it's of the whole frame, uncropped and turned
    /// as `orientation`: what the crop tool draws the crop over.
    pub fn whole(&self, path: &Path, orientation: Orientation) -> Option<&TextureHandle> {
        let render = self.shown.get(path)?.render.as_ref()?;
        (render.settings.crop == Default::default() && render.settings.orientation == orientation).then_some(&render.texture)
    }

    /// Draw `path`'s frame: its render, or the camera's JPEG until that's
    /// in; whole, or at 100%. With [`Mouse::Pick`] (the White Balance
    /// Selector) the pointer is a crosshair over the frame, and a click on
    /// it is returned, as a share of the frame's width and height; with
    /// [`Mouse::Zoom`], the mouse zooms and pans. With [`Mouse::Tool`]
    /// only the room is taken, and nothing drawn. Also returns the loupe's
    /// response, for its right-click menu, but under the crop tool.
    pub fn show(&mut self, ui: &mut Ui, path: &Path, mouse: Mouse) -> (Option<[f64; 2]>, Option<egui::Response>) {
        let pixels_per_point = ui.ctx().pixels_per_point();
        self.area = ui.max_rect().shrink(MARGIN);
        let pixels = self.area.size() * pixels_per_point;
        self.size = (pixels.x.max(0.0) as usize, pixels.y.max(0.0) as usize);
        if mouse == Mouse::Tool {
            (self.zoomed, self.hold) = (false, None);
            return (None, None);
        }
        let menu = || ui.interact(self.area, ui.id().with("loupe menu"), egui::Sense::click());
        let texture = self.shown.get(path).and_then(|shown| shown.render.as_ref().map(|r| &r.texture).or(shown.preview.as_ref()));
        let (Some(shown), Some(texture)) = (self.shown.get(path), texture) else {
            (self.fit, self.full) = (Rect::NOTHING, Vec2::ZERO);
            return (None, Some(menu()));
        };
        self.fit = fit(texture.size_vec2(), self.area);
        self.full = shown.full.map_or(Vec2::ZERO, |(w, h)| vec2(w as f32, h as f32) / pixels_per_point);
        let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        let painter = ui.painter_at(self.area);
        let frame = if self.zoomed && self.full.x > 0.0 {
            // The render that fits, enlarged, under the window rendered at
            // full size: it covers what that window doesn't, or all of it
            // until the window is in.
            let frame = self.zoomed_rect(pixels_per_point);
            painter.image(texture.id(), frame, uv, egui::Color32::WHITE);
            if let Some(zoomed) = &shown.zoomed {
                let [x, y, w, h] = zoomed.key.window.map(|v| v as f32 / pixels_per_point);
                painter.image(zoomed.texture.id(), Rect::from_min_size(frame.min + vec2(x, y), vec2(w, h)), uv, egui::Color32::WHITE);
            }
            frame
        } else {
            painter.image(texture.id(), self.fit, uv, egui::Color32::WHITE);
            self.fit
        };
        #[cfg(test)]
        ui.data_mut(|d| d.insert_temp(egui::Id::new("loupe frame"), frame));
        let on_frame = frame.intersect(self.area);
        if mouse == Mouse::Pick {
            let response = ui.interact(on_frame, ui.id().with("white balance selector"), egui::Sense::click()).on_hover_cursor(egui::CursorIcon::Crosshair);
            let at = response.clicked().then(|| response.interact_pointer_pos()).flatten();
            let at = at.map(|at| [f64::from((at.x - frame.left()) / frame.width()), f64::from((at.y - frame.top()) / frame.height())]);
            return (at, Some(response));
        }
        if self.full.x > 0.0 {
            let response = ui.interact(self.area, ui.id().with("loupe"), egui::Sense::click_and_drag());
            self.pointer(ui, &response);
            return (None, Some(response));
        }
        (None, Some(menu()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A loupe 600 by 400 with a frame of 3000 by 2000 fitted in it.
    fn loupe() -> Loupe {
        let area = Rect::from_min_size(egui::pos2(100.0, 50.0), vec2(600.0, 400.0));
        Loupe { area, fit: area, full: vec2(3000.0, 2000.0), ..Loupe::default() }
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer() {
        let mut l = loupe();
        // A point a quarter of the way in, right of the loupe's middle.
        let at = egui::pos2(100.0 + 150.0, 50.0 + 100.0);
        l.zoom_in(Some(at));
        let frame = l.zoomed_rect(1.0);
        let under = (at - frame.min) / frame.size();
        assert!((under.x - 0.25).abs() < 1e-3 && (under.y - 0.25).abs() < 1e-3, "{under:?}");
        assert_eq!(l.zoom().map(|[u, v]| [(u * 100.0).round(), (v * 100.0).round()]), Some([30.0, 30.0]));
    }

    #[test]
    fn the_frame_covers_the_loupe_at_its_edges_and_panning_stops_there() {
        let mut l = loupe();
        l.zoom_in(Some(egui::pos2(100.0, 50.0)));
        assert_eq!(l.zoomed_rect(1.0).min, l.area.min, "zoomed at the corner, the frame's corner is the loupe's");
        l.pan(vec2(500.0, 0.0));
        assert_eq!(l.zoomed_rect(1.0).min, l.area.min, "there's nothing further left to see");
        l.pan(vec2(-300.0, -200.0));
        assert_eq!(l.zoomed_rect(1.0).min, l.area.min - vec2(300.0, 200.0));
        l.pan(vec2(-9000.0, -9000.0));
        assert_eq!(l.zoomed_rect(1.0).max, l.area.max);
    }

    #[test]
    fn a_tap_toggles_and_a_hold_is_a_look() {
        let mut l = loupe();
        l.key_down(None, 1.0);
        l.key_down(None, 1.05);
        l.key_up(1.1);
        assert!(l.zoomed, "a tap, its repeats ignored");
        l.key_down(None, 2.0);
        l.key_up(2.1);
        assert!(!l.zoomed);
        l.key_down(None, 3.0);
        l.key_up(3.1 + HOLD);
        assert!(!l.zoomed, "held, it was a look");
        // Nothing to zoom to until the frame's size at 100% is known.
        let mut unknown = Loupe { full: Vec2::ZERO, ..loupe() };
        unknown.key_down(None, 1.0);
        assert!(unknown.zoom().is_none() && !unknown.zoomed);
    }
}
