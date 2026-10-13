//! The loupe: the current frame, as large as fits. What it shows of each
//! frame the renderer was asked for: the camera's embedded JPEG at once,
//! and the render once it's in.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{Rect, TextureHandle, TextureOptions, Ui, Vec2};
use lightcraft_develop::DevelopSettings;
use lightcraft_pipeline::SourceInfo;
use lightcraft_raster::Histogram;

use crate::render::{Done, Key};

/// The pasteboard left round the frame, in points.
const MARGIN: f32 = 12.0;

/// A frame's render on screen.
pub struct Render {
    pub texture: TextureHandle,
    pub histogram: Histogram,
    /// Which render it is, and the settings it was rendered with: only
    /// the tests ask, so far.
    #[cfg_attr(not(test), allow(dead_code))]
    pub key: Key,
    #[cfg_attr(not(test), allow(dead_code))]
    pub settings: Arc<DevelopSettings>,
}

/// What the renderer finished that isn't a picture.
pub enum News {
    /// A frame's raw is decoded.
    Decoded(PathBuf, Arc<SourceInfo>),
    /// A frame couldn't be developed, and why.
    Failed(PathBuf, String),
}

/// What there is to show of a frame.
#[derive(Default)]
pub struct Shown {
    /// The camera's JPEG.
    pub preview: Option<TextureHandle>,
    pub render: Option<Render>,
}

#[derive(Default)]
pub struct Loupe {
    shown: HashMap<PathBuf, Shown>,
    /// The room the frame had when last drawn, in pixels: what renders
    /// are asked for.
    size: (usize, usize),
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

    #[cfg(test)]
    pub fn shown(&self, path: &Path) -> Option<&Shown> {
        self.shown.get(path)
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
                Done::Decoded { path, info } => news.push(News::Decoded(path, info)),
                Done::Failed { path, error } => news.push(News::Failed(path, error)),
            }
        }
        news
    }

    /// Draw `path`'s frame: its render, or the camera's JPEG until that's
    /// in. With `picking` (the White Balance Selector) the pointer is a
    /// crosshair over the frame, and a click on it is returned, as a share
    /// of the frame's width and height.
    pub fn show(&mut self, ui: &mut Ui, path: &Path, picking: bool) -> Option<[f64; 2]> {
        let area = ui.max_rect().shrink(MARGIN);
        let pixels = area.size() * ui.ctx().pixels_per_point();
        self.size = (pixels.x.max(0.0) as usize, pixels.y.max(0.0) as usize);
        let shown = self.shown.get(path)?;
        let texture = shown.render.as_ref().map(|r| &r.texture).or(shown.preview.as_ref())?;
        let frame = fit(texture.size_vec2(), area);
        let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        ui.painter().image(texture.id(), frame, uv, egui::Color32::WHITE);
        #[cfg(test)]
        ui.data_mut(|d| d.insert_temp(egui::Id::new("loupe frame"), frame));
        if !picking {
            return None;
        }
        let response = ui.interact(frame, ui.id().with("white balance selector"), egui::Sense::click()).on_hover_cursor(egui::CursorIcon::Crosshair);
        let at = response.clicked().then(|| response.interact_pointer_pos()).flatten()?;
        Some([f64::from((at.x - frame.left()) / frame.width()), f64::from((at.y - frame.top()) / frame.height())])
    }
}
