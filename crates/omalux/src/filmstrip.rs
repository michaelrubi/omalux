//! The filmstrip under the loupe: thumbnails of the shown frames in a row
//! that scrolls sideways and keeps the current frame in view.
//!
//! Lifted from Omacull (`omacull/src/shoot.rs`).
//!
//! Thumbnails are loaded off the UI thread via [`omalux_engine::thumbs::Loader`].
//! Textures are uploaded to egui as thumbnails arrive and kept only for frames
//! near the current view.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use egui::{
    Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle, Ui, Vec2, pos2, vec2,
};
use lightcraft_codecs::display::DisplayProfile;
use omalux_engine::shoot::{self, Rating, Shoot, stars};
use omalux_engine::sidecar;
use omalux_engine::thumbs::{Loaded, Loader};

use crate::theme::Theme;

/// The filmstrip's height, matching Omacull.
pub const STRIP_HEIGHT: f32 = 104.0;

/// Thumbnails further than this from the current frame are dropped from the GPU.
const KEEP_THUMBNAILS: usize = 100;

/// A thumbnail on the GPU, or one that couldn't be made.
pub enum Slot {
    Ready(TextureHandle),
    Failed,
}

/// How a filmstrip click was meant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    Plain,
    /// Ctrl: in or out of the selection.
    Toggle,
    /// Shift: the selection runs to here.
    Range,
}

/// A shoot's filmstrip: its thumbnails' loader, the ones on the GPU, and
/// where each frame was last drawn.
pub struct Filmstrip {
    loader: Option<Loader>,
    thumbnails: HashMap<usize, Slot>,
    wanted: Vec<usize>,
    /// The frames in the strip and where they were last drawn.
    pub cells: Vec<(usize, Rect)>,
    /// Where thumbnails are cached; nowhere in tests.
    cache: Option<PathBuf>,
    display: Option<Arc<DisplayProfile>>,
}

impl Filmstrip {
    pub fn new(cache: Option<PathBuf>) -> Self {
        Self { loader: None, thumbnails: HashMap::new(), wanted: Vec::new(), cells: Vec::new(), cache, display: None }
    }

    /// Forget the shoot: another is opened, or this one closed.
    pub fn reset(&mut self) {
        let display = self.display.clone();
        *self = Self::new(self.cache.take());
        self.display = display;
    }

    /// Show thumbnails in another monitor's colours. Dropped and loaded
    /// again when the profile changes.
    pub fn set_display(&mut self, display: Option<Arc<DisplayProfile>>) {
        let changed = match (&self.display, &display) {
            (Some(a), Some(b)) => a.id != b.id,
            (None, None) => false,
            _ => true,
        };
        if changed {
            self.thumbnails.clear();
            self.wanted.clear();
            self.display = display;
        }
    }

    /// Start loading the open shoot's thumbnails, if that hasn't begun.
    pub fn sync_shoot(&mut self, shoot: &Shoot, ctx: &egui::Context) {
        if self.loader.is_some() {
            return;
        }
        let raws = shoot.frames().iter().map(|f| f.path.clone()).collect();
        let ctx = ctx.clone();
        self.loader = Some(Loader::new(raws, self.cache.clone(), move || ctx.request_repaint()));
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
                    Ok(mut image) => {
                        if let Some(display) = &self.display {
                            let _ = display.from_srgb(&mut image);
                        }
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
                        log::debug!("no thumbnail for frame {index}: {e}");
                        self.thumbnails.insert(index, Slot::Failed);
                    }
                }
            }
        }
    }

    /// Draw the filmstrip: thumbnails in a row that scrolls sideways and
    /// keeps the current frame in view. Returns the frame clicked, and how.
    pub fn show(&mut self, ui: &mut Ui, shoot: &Shoot, theme: &Theme) -> Option<(usize, Click)> {
        let (strip, response) = ui.allocate_exact_size(vec2(ui.available_width(), STRIP_HEIGHT), Sense::click());
        let painter = ui.painter_at(strip);
        let cell = vec2(STRIP_HEIGHT * 1.5, STRIP_HEIGHT);
        let reach = (strip.width() / cell.x / 2.0).ceil() as usize + 1;
        let current = shoot.current();
        let shown = shoot.shown_indices();
        let at = shown.iter().position(|&i| i == current).unwrap_or(0);
        self.cells.clear();

        let visible_range = at.saturating_sub(reach)..=(at + reach).min(shown.len().saturating_sub(1));
        for (k, &i) in shown.iter().enumerate() {
            if !visible_range.contains(&k) {
                continue;
            }
            let x = strip.center().x + (k as f32 - at as f32) * cell.x;
            let rect = Rect::from_center_size(pos2(x, strip.center().y), cell);
            self.cells.push((i, rect));
            let rating = shoot.frames()[i].rating;

            if i == current || shoot.is_selected(i) {
                painter.rect_filled(rect.shrink(1.0), 0.0, theme.selection);
            }
            if i == current {
                painter.rect_stroke(rect.shrink(2.0), 0.0, Stroke::new(2.0, theme.accent), StrokeKind::Inside);
            }

            let inner = rect.shrink(6.0);
            let stack = shoot.stack(i).map(|s| s.len());
            let collapsed = stack.is_some() && !shoot.expanded(i);
            if collapsed {
                // Cards behind, for the frames it stands for.
                for offset in [4.0, 2.0] {
                    let card = inner.translate(vec2(offset, -offset));
                    painter.rect_filled(card, 1.0, theme.lighter_background);
                    painter.rect_stroke(card, 1.0, Stroke::new(1.0, theme.dark_foreground), StrokeKind::Inside);
                }
            }
            match self.thumbnails.get(&i) {
                Some(Slot::Ready(texture)) => {
                    let image = fit(texture.size_vec2(), inner);
                    let tint = if rating == sidecar::REJECT { Color32::from_gray(80) } else { Color32::WHITE };
                    painter.image(texture.id(), image, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), tint);
                }
                Some(Slot::Failed) => {
                    painter.rect_filled(inner, 2.0, theme.darker_background);
                    painter.text(inner.center(), Align2::CENTER_CENTER, "!", FontId::proportional(14.0), theme.red);
                }
                None => {
                    painter.rect_filled(inner, 2.0, theme.darker_background);
                }
            }
            badge(&painter, inner.center_bottom() - vec2(0.0, 3.0), Align2::CENTER_BOTTOM, rating, 13.0, theme);
            match stack {
                Some(n) if collapsed => {
                    let at = inner.right_top() + vec2(-3.0, 3.0);
                    label(&painter, at, Align2::RIGHT_TOP, format!("{n}"), theme.foreground, 13.0);
                }
                // An open stack's frames are underlined together.
                Some(_) => {
                    let line = [rect.left_bottom() + vec2(0.0, -2.0), rect.right_bottom() + vec2(0.0, -2.0)];
                    painter.line_segment(line, Stroke::new(3.0, theme.accent));
                }
                None => {}
            }
        }

        // Ask the loader for what's in or near view, closest to current first.
        let reach_range = at.saturating_sub(reach + 2)..(at + reach + 3).min(shown.len());
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
        let modifiers = ui.input(|i| i.modifiers);
        let click = match () {
            () if modifiers.command => Click::Toggle,
            () if modifiers.shift => Click::Range,
            () => Click::Plain,
        };
        self.cells.iter().find(|(_, rect)| rect.contains(pointer)).map(|&(i, _)| (i, click))
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

#[cfg(test)]
mod tests {
    use super::*;
    use lightcraft_raster::Rgba8;

    #[test]
    fn thumbnails_converted_with_profile_and_left_alone_without() {
        // P3 EDID
        let mut edid = vec![0u8; 128];
        edid[..8].copy_from_slice(&[0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0]);
        edid[23] = 120;
        edid[27..35].copy_from_slice(&[174, 82, 61, 185, 36, 13, 80, 84]);
        let p3 = omalux_engine::color::display(None, Some(&edid)).unwrap().unwrap();

        let mut img_srgb = Rgba8::filled(1, 1, [255, 0, 0, 255]);
        let mut img_p3 = img_srgb.clone();

        // Without profile (sRGB): pixels stay as they are
        let display_none: Option<&DisplayProfile> = None;
        if let Some(display) = display_none {
            let _ = display.from_srgb(&mut img_srgb);
        }
        assert_eq!(img_srgb.data[0], [255, 0, 0, 255]);

        // With profile: pixels are converted (P3 red drive is lower)
        p3.from_srgb(&mut img_p3).unwrap();
        assert!(img_p3.data[0][0] < 240 && img_p3.data[0][1] > 20, "{:?}", img_p3.data[0]);
    }

    #[test]
    fn profile_change_drops_thumbnails_and_wanted() {
        let mut edid = vec![0u8; 128];
        edid[..8].copy_from_slice(&[0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0]);
        edid[23] = 120;
        edid[27..35].copy_from_slice(&[174, 82, 61, 185, 36, 13, 80, 84]);
        let p3 = Arc::new(omalux_engine::color::display(None, Some(&edid)).unwrap().unwrap());

        let mut strip = Filmstrip::new(None);
        strip.wanted = vec![0, 1, 2];
        strip.thumbnails.insert(0, Slot::Failed);

        // Setting same profile (None -> None) changes nothing
        strip.set_display(None);
        assert_eq!(strip.wanted, vec![0, 1, 2]);
        assert_eq!(strip.thumbnails.len(), 1);

        // Setting a new profile drops thumbnails and wanted
        strip.set_display(Some(p3.clone()));
        assert!(strip.wanted.is_empty());
        assert!(strip.thumbnails.is_empty());

        // Setting the same profile again does not drop
        strip.wanted = vec![3];
        strip.set_display(Some(p3));
        assert_eq!(strip.wanted, vec![3]);
    }
}
