//! The filmstrip under the loupe: thumbnails of the shown frames in a row
//! that scrolls sideways and keeps the current frame in view.
//!
//! Lifted from Omacull (`omacull/src/shoot.rs`).
//!
//! Thumbnails are loaded off the UI thread via [`omalux_engine::thumbs::Loader`].
//! Textures are uploaded to egui as thumbnails arrive and kept only for frames
//! near the current view.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use egui::{
    Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle, Ui, Vec2, pos2, vec2,
};
use omalux_engine::shoot::{self, Rating, Shoot};
use omalux_engine::sidecar;
use omalux_engine::thumbs::{self, Loaded, Loader};

use crate::theme::Theme;

/// The filmstrip's height, matching Omacull.
pub const STRIP_HEIGHT: f32 = 104.0;

/// Thumbnails further than this from the current frame are dropped from the GPU.
const KEEP_THUMBNAILS: usize = 100;

/// Something decoded and on the GPU, or why it isn't.
#[derive(Clone, Debug)]
pub enum Slot<T> {
    Ready(T),
    Failed(#[allow(dead_code)] String),
}

/// The filmstrip state: thumbnail loader, texture cache, and layout cells.
pub struct Filmstrip {
    pub loader: Option<Loader>,
    pub thumbnails: HashMap<usize, Slot<TextureHandle>>,
    pub cached: HashSet<usize>,
    pub wanted: Vec<usize>,
    pub reach: usize,
    /// The filmstrip's frames and where they were last drawn.
    pub cells: Vec<(usize, Rect)>,
    dir: Option<PathBuf>,
}

impl Default for Filmstrip {
    fn default() -> Self {
        Self::new()
    }
}

impl Filmstrip {
    pub fn new() -> Self {
        Self {
            loader: None,
            thumbnails: HashMap::new(),
            cached: HashSet::new(),
            wanted: Vec::new(),
            reach: 0,
            cells: Vec::new(),
            dir: None,
        }
    }

    /// Keep the loader in sync with the open shoot.
    pub fn sync_shoot(&mut self, shoot: &Shoot, ctx: &egui::Context) {
        let same_dir = self.dir.as_deref() == Some(shoot.dir());
        if same_dir && self.loader.is_some() {
            return;
        }
        self.dir = Some(shoot.dir().to_path_buf());
        self.thumbnails.clear();
        self.cached.clear();
        self.wanted.clear();
        self.cells.clear();
        let raws = shoot.frames().iter().map(|f| f.path.clone()).collect();
        let ctx = ctx.clone();
        self.loader = Some(Loader::new(raws, thumbs::default_dir(), move || ctx.request_repaint()));
    }

    /// Collect thumbnails that finished loading off the UI thread, and drop
    /// textures that are no longer near view.
    pub fn collect(&mut self, ctx: &egui::Context, current: usize) {
        let near = |i: &usize| i.abs_diff(current) <= KEEP_THUMBNAILS;
        self.thumbnails.retain(|i, _| near(i));
        let Some(loader) = &self.loader else { return };
        for Loaded { index, result } in loader.loaded() {
            if near(&index) {
                match result {
                    Ok(image) => {
                        self.cached.insert(index);
                        let pixels = egui::ColorImage::from_rgba_unmultiplied(
                            [image.width, image.height],
                            image.data.as_flattened(),
                        );
                        let texture = ctx.load_texture(
                            format!("thumbnail-{index}"),
                            pixels,
                            egui::TextureOptions::LINEAR,
                        );
                        self.thumbnails.insert(index, Slot::Ready(texture));
                    }
                    Err(e) => {
                        self.thumbnails.insert(index, Slot::Failed(e.to_string()));
                    }
                }
            }
        }
    }

    /// Draw the filmstrip: thumbnails in a row that scrolls sideways and
    /// keeps the current frame in view. Returns the frame clicked, if any.
    pub fn show(&mut self, ui: &mut Ui, shoot: &Shoot, theme: &Theme) -> Option<usize> {
        let (strip, response) = ui.allocate_exact_size(vec2(ui.available_width(), STRIP_HEIGHT), Sense::click());
        let painter = ui.painter_at(strip);
        let cell = vec2(STRIP_HEIGHT * 1.5, STRIP_HEIGHT);
        self.reach = (strip.width() / cell.x / 2.0).ceil() as usize + 1;
        let current = shoot.current();
        let shown = shoot.shown_indices();
        let at = shown.iter().position(|&i| i == current).unwrap_or(0);
        self.cells.clear();

        let visible_range = at.saturating_sub(self.reach)..=(at + self.reach).min(shown.len().saturating_sub(1));
        for (k, &i) in shown.iter().enumerate() {
            if !visible_range.contains(&k) {
                continue;
            }
            let x = strip.center().x + (k as f32 - at as f32) * cell.x;
            let rect = Rect::from_center_size(pos2(x, strip.center().y), cell);
            self.cells.push((i, rect));
            let rating = shoot.frames()[i].rating;

            if i == current {
                painter.rect_filled(rect.shrink(1.0), 0.0, theme.selection);
                painter.rect_stroke(rect.shrink(2.0), 0.0, Stroke::new(2.0, theme.accent), StrokeKind::Inside);
            }

            let inner = rect.shrink(6.0);
            match self.thumbnails.get(&i) {
                Some(Slot::Ready(texture)) => {
                    let image = fit(texture.size_vec2(), inner);
                    let tint = if rating == sidecar::REJECT { Color32::from_gray(80) } else { Color32::WHITE };
                    painter.image(texture.id(), image, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), tint);
                }
                Some(Slot::Failed(_)) => {
                    painter.rect_filled(inner, 2.0, theme.darker_background);
                    painter.text(inner.center(), Align2::CENTER_CENTER, "!", FontId::proportional(14.0), theme.red);
                }
                None => {
                    painter.rect_filled(inner, 2.0, theme.darker_background);
                }
            }
            badge(&painter, inner.center_bottom() - vec2(0.0, 3.0), Align2::CENTER_BOTTOM, rating, 13.0, theme);
        }

        // Ask the loader for what's in or near view, closest to current first.
        let reach_range = at.saturating_sub(self.reach + 2)..(at + self.reach + 3).min(shown.len());
        let mut strip_frames: Vec<usize> = shown[reach_range].to_vec();
        strip_frames.sort_by_key(|&i| i.abs_diff(current));
        let wanted: Vec<usize> = strip_frames.into_iter().filter(|&i| !self.thumbnails.contains_key(&i)).collect();
        if wanted != self.wanted {
            if let Some(loader) = &self.loader {
                loader.want(wanted.iter().copied());
            }
            self.wanted = wanted;
        }

        let pointer = response.interact_pointer_pos().filter(|_| response.clicked())?;
        self.cells.iter().find(|(_, rect)| rect.contains(pointer)).map(|&(i, _)| i)
    }
}

/// An image sized `size` fitted within `area`, centered.
fn fit(size: Vec2, area: Rect) -> Rect {
    if size.x <= 0.0 || size.y <= 0.0 {
        return area;
    }
    let scale = (area.width() / size.x).min(area.height() / size.y);
    Rect::from_center_size(area.center(), size * scale)
}

fn stars(n: Rating) -> String {
    "★".repeat(n.clamp(1, 5) as usize)
}

/// A frame's mark badge on a dark pill plate. Nothing for an undecided frame.
fn badge(painter: &egui::Painter, at: Pos2, align: Align2, rating: Rating, size: f32, theme: &Theme) {
    let (text, colour) = match rating {
        0 => return,
        sidecar::REJECT if size > 14.0 => ("✕ Rejected".to_owned(), theme.red),
        sidecar::REJECT => ("✕".to_owned(), theme.red),
        shoot::PICK if size > 14.0 => ("★ Pick".to_owned(), theme.accent),
        stars_ => (stars(stars_), theme.accent),
    };
    label(painter, at, align, text, colour, size);
}

fn label(painter: &egui::Painter, at: Pos2, align: Align2, text: String, colour: Color32, size: f32) {
    let galley = painter.layout_no_wrap(text, FontId::proportional(size), colour);
    let rect = align.anchor_size(at, galley.size()).expand2(vec2(size * 0.35, size * 0.15));
    painter.rect_filled(rect, size * 0.2, Color32::from_black_alpha(170));
    painter.galley(align.anchor_size(at, galley.size()).min, galley, colour);
}
