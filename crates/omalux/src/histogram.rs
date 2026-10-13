//! The histogram at the top of the develop panel, and clipping warnings.
//!
//! Lifted from Lightcraft (MIT OR Apache-2.0) at `701f78e`,
//! `crates/ui-egui/src/panels/edit.rs` (`histogram`): restyled to Omarchy's
//! theme, drawing channels blended where they overlap in Lightroom's look,
//! and with interactive clipping warnings (J).

use egui::{Color32, Painter, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};
use lightcraft_raster::{Histogram, Rgba8};

use crate::theme::Theme;

/// The height of the histogram plate in points.
pub const HISTOGRAM_HEIGHT: f32 = 118.0;

/// Fraction of pixels in bin 0 or 255 for a channel to count as clipped
/// (Lightcraft's threshold from `edit.rs`: 0.3% of the photo's pixels).
pub const CLIPPING_THRESHOLD: f32 = 0.003;

/// Blown highlights baked into the overlay: red (Omacull's loupe color).
#[allow(dead_code)]
pub const HIGHLIGHT: [u8; 4] = [255, 0, 0, 255];

/// Crushed shadows baked into the overlay: blue (Omacull's loupe color).
#[allow(dead_code)]
pub const SHADOW: [u8; 4] = [0, 90, 255, 255];

/// Channel identity for blending layers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Channel {
    R,
    G,
    B,
}

/// The color a clipping triangle lights up with, given which channels clip.
///
/// In Lightroom: white when all three channels clip, otherwise the primary or
/// secondary color of the clipped channels (red, green, blue, yellow, magenta, cyan).
pub fn clip_color(r: bool, g: bool, b: bool) -> Option<Color32> {
    match (r, g, b) {
        (true, true, true) => Some(Color32::WHITE),
        (true, true, false) => Some(Color32::from_rgb(255, 255, 0)),
        (true, false, true) => Some(Color32::from_rgb(255, 0, 255)),
        (false, true, true) => Some(Color32::from_rgb(0, 255, 255)),
        (true, false, false) => Some(Color32::from_rgb(255, 0, 0)),
        (false, true, false) => Some(Color32::from_rgb(0, 255, 0)),
        (false, false, true) => Some(Color32::from_rgb(0, 0, 255)),
        (false, false, false) => None,
    }
}

/// Which clipping triangles light up and in what color: `(shadows, highlights)`.
///
/// Each triangle lights up when that channel's pixel count exceeds Lightcraft's
/// threshold of 0.3% ([`CLIPPING_THRESHOLD`]) of the total pixels: shadows at
/// bin 0, highlights at bin 255. White when all three channels clip, otherwise
/// the color of the channels that do.
pub fn clipping_triangles(histogram: &Histogram) -> (Option<Color32>, Option<Color32>) {
    if histogram.total == 0 {
        return (None, None);
    }
    let t = histogram.total as f32;
    let is_clipped = |bin: Option<&u32>| bin.copied().unwrap_or(0) as f32 / t > CLIPPING_THRESHOLD;

    let shadow = clip_color(
        is_clipped(histogram.r.first()),
        is_clipped(histogram.g.first()),
        is_clipped(histogram.b.first()),
    );
    let highlight = clip_color(
        is_clipped(histogram.r.last()),
        is_clipped(histogram.g.last()),
        is_clipped(histogram.b.last()),
    );
    (shadow, highlight)
}

/// Apply the clipping overlay in place: pixels with any channel at 255 become
/// red, and pixels with every channel at 0 become blue.
#[allow(dead_code)]
pub fn apply_clipping_overlay(image: &mut Rgba8) {
    for p in &mut image.data {
        if p[0] == 255 || p[1] == 255 || p[2] == 255 {
            *p = [HIGHLIGHT[0], HIGHLIGHT[1], HIGHLIGHT[2], p[3]];
        } else if p[0] == 0 && p[1] == 0 && p[2] == 0 {
            *p = [SHADOW[0], SHADOW[1], SHADOW[2], p[3]];
        }
    }
}

/// The image with the clipping overlay applied: pixels with any channel at 255
/// become red, and pixels with every channel at 0 become blue.
#[allow(dead_code)]
pub fn clipping_overlay(mut image: Rgba8) -> Rgba8 {
    apply_clipping_overlay(&mut image);
    image
}

/// Helper returning a copy of the image with the overlay applied.
#[allow(dead_code)]
pub fn overlay(image: &Rgba8) -> Rgba8 {
    clipping_overlay(image.clone())
}

/// Smooth a 256-bin channel with a 5-wide moving average, as Lightcraft does.
fn smooth(v: &[u32]) -> [f32; 256] {
    let mut out = [0.0; 256];
    for (i, slot) in out.iter_mut().enumerate() {
        let a = i.saturating_sub(2);
        let b = (i + 3).min(256);
        let sum: u32 = v[a..b].iter().sum();
        *slot = sum as f32 / (b - a) as f32;
    }
    out
}

fn dual_color(a: Channel, b: Channel) -> Color32 {
    match (a, b) {
        (Channel::R, Channel::G) | (Channel::G, Channel::R) => Color32::from_rgb(218, 190, 55),
        (Channel::R, Channel::B) | (Channel::B, Channel::R) => Color32::from_rgb(200, 60, 170),
        (Channel::G, Channel::B) | (Channel::B, Channel::G) => Color32::from_rgb(55, 180, 200),
        _ => Color32::from_gray(140),
    }
}

fn single_color(c: Channel) -> Color32 {
    match c {
        Channel::R => Color32::from_rgb(223, 57, 57),
        Channel::G => Color32::from_rgb(68, 176, 114),
        Channel::B => Color32::from_rgb(59, 111, 224),
    }
}

/// Draw the histogram plate, channels, and clipping warning triangles.
///
/// Returns `true` if either clipping triangle was clicked.
pub fn show(ui: &mut Ui, theme: &Theme, histogram: Option<&Histogram>, clipping: bool) -> bool {
    let w = ui.available_width();
    let (rect, _resp) = ui.allocate_exact_size(vec2(w, HISTOGRAM_HEIGHT), Sense::hover());
    let painter = ui.painter();

    // The plate.
    painter.rect_filled(rect, 0.0, theme.darker_background);

    let Some(h) = histogram else {
        return false;
    };

    let plot = Rect::from_min_max(rect.min + vec2(0.0, 6.0), rect.max - vec2(0.0, 6.0));
    if plot.width() <= 0.0 || plot.height() <= 0.0 {
        return false;
    }

    let smooth_r = smooth(&h.r);
    let smooth_g = smooth(&h.g);
    let smooth_b = smooth(&h.b);

    // Peak ignores the two bins on each edge so tall clipping spikes don't flatten the rest.
    let peak = [smooth_r, smooth_g, smooth_b]
        .iter()
        .flat_map(|v| v[2..254].iter().copied())
        .fold(1.0f32, f32::max);

    let to_height = |val: f32| -> f32 { (val / peak).sqrt().min(1.0) * plot.height() };
    let to_pos = |i: usize, val: f32| -> Pos2 {
        pos2(
            plot.left() + plot.width() * i as f32 / 255.0,
            plot.bottom() - to_height(val),
        )
    };

    // Draw blended vertical strips for the three channels.
    for i in 0..256 {
        let x0 = plot.left() + plot.width() * i as f32 / 256.0;
        let x1 = (plot.left() + plot.width() * (i + 1) as f32 / 256.0 + 0.5).min(plot.right());
        if x1 <= x0 {
            continue;
        }

        let r_h = to_height(smooth_r[i]);
        let g_h = to_height(smooth_g[i]);
        let b_h = to_height(smooth_b[i]);

        let mut chs = [(Channel::R, r_h), (Channel::G, g_h), (Channel::B, b_h)];
        chs.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let (_, h0) = chs[0];
        let (c1, h1) = chs[1];
        let (c2, h2) = chs[2];

        // 1. Where all three channels overlap: neutral grey.
        if h0 > 0.5 {
            let r = Rect::from_min_max(pos2(x0, plot.bottom() - h0), pos2(x1, plot.bottom()));
            painter.rect_filled(r, 0.0, Color32::from_gray(140));
        }

        // 2. Where two channels overlap: secondary color.
        if h1 - h0 > 0.5 {
            let r = Rect::from_min_max(pos2(x0, plot.bottom() - h1), pos2(x1, plot.bottom() - h0));
            painter.rect_filled(r, 0.0, dual_color(c1, c2));
        }

        // 3. Where only one channel reaches: primary color.
        if h2 - h1 > 0.5 {
            let r = Rect::from_min_max(pos2(x0, plot.bottom() - h2), pos2(x1, plot.bottom() - h1));
            painter.rect_filled(r, 0.0, single_color(c2));
        }
    }

    // Top channel stroke outlines (Lightcraft: neutral line when greyscale).
    let spread: f32 = (0..256).map(|i| (smooth_r[i] - smooth_g[i]).abs() + (smooth_g[i] - smooth_b[i]).abs()).sum();
    let total: f32 = smooth_g.iter().sum::<f32>().max(1.0);
    if spread / total < 0.02 {
        let pts: Vec<Pos2> = (0..256).map(|i| to_pos(i, smooth_g[i])).collect();
        painter.add(Shape::line(pts, Stroke::new(1.5, Color32::from_gray(200))));
    } else {
        for (v, c) in [
            (&smooth_r, Color32::from_rgb(223, 57, 57)),
            (&smooth_g, Color32::from_rgb(68, 176, 114)),
            (&smooth_b, Color32::from_rgb(59, 111, 224)),
        ] {
            let pts: Vec<Pos2> = (0..256).map(|i| to_pos(i, v[i])).collect();
            painter.add(Shape::line(pts, Stroke::new(1.2, c)));
        }
    }

    // Clipping triangles.
    let (shadow_color, highlight_color) = clipping_triangles(h);
    let left_center = pos2(plot.left() + 14.0, plot.top() + 12.0);
    let right_center = pos2(plot.right() - 14.0, plot.top() + 12.0);
    let tri_size = vec2(18.0, 18.0);
    let left_rect = Rect::from_center_size(left_center, tri_size);
    let right_rect = Rect::from_center_size(right_center, tri_size);

    let left_resp = ui.interact(left_rect, ui.id().with("shadow_clip_tri"), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Shadow clipping (J)");
    let right_resp = ui.interact(right_rect, ui.id().with("highlight_clip_tri"), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Highlight clipping (J)");

    let triangle_clicked = left_resp.clicked() || right_resp.clicked();

    let draw_triangle_button = |p: &Painter, btn_rect: Rect, hovered: bool, lit: Option<Color32>| {
        let bg = if hovered {
            theme.lighter_background
        } else if clipping {
            theme.selection
        } else {
            Color32::from_black_alpha(140)
        };
        p.rect_filled(btn_rect, 2.0, bg);
        if clipping {
            p.rect_stroke(btn_rect, 2.0, Stroke::new(1.0, theme.foreground), egui::StrokeKind::Inside);
        }

        let glyph_color = lit.unwrap_or(theme.dark_foreground);
        let c = btn_rect.center();
        let half_w = 4.5;
        let half_h = 4.0;
        let top = pos2(c.x, c.y - half_h);
        let br = pos2(c.x + half_w, c.y + half_h);
        let bl = pos2(c.x - half_w, c.y + half_h);
        p.add(Shape::convex_polygon(vec![top, br, bl], glyph_color, Stroke::NONE));
    };

    draw_triangle_button(painter, left_rect, left_resp.hovered(), shadow_color);
    draw_triangle_button(painter, right_rect, right_resp.hovered(), highlight_color);

    triangle_clicked
}

/// An alias for [`show`] matching the widget's name in `edit.rs`.
#[allow(dead_code)]
pub fn histogram(ui: &mut Ui, theme: &Theme, histogram: Option<&Histogram>, clipping: bool) -> bool {
    show(ui, theme, histogram, clipping)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_histogram(shadows: [u32; 3], highlights: [u32; 3], total: u32) -> Histogram {
        let mut r = vec![0; 256];
        let mut g = vec![0; 256];
        let mut b = vec![0; 256];
        let luma = vec![0; 256];
        r[0] = shadows[0];
        g[0] = shadows[1];
        b[0] = shadows[2];
        r[255] = highlights[0];
        g[255] = highlights[1];
        b[255] = highlights[2];
        Histogram { r, g, b, luma, total, hdr: None }
    }

    #[test]
    fn hand_crafted_histogram_triangles() {
        let total = 1000;
        // 4 / 1000 = 0.004 > 0.003 (clips). 2 / 1000 = 0.002 <= 0.003 (does not clip).
        let clip = 4;
        let safe = 2;

        // Clean: neither clips.
        let h_clean = make_histogram([safe, safe, safe], [safe, safe, safe], total);
        assert_eq!(clipping_triangles(&h_clean), (None, None));

        // Shadows: all three clip -> White.
        let h_sh_white = make_histogram([clip, clip, clip], [0, 0, 0], total);
        assert_eq!(clipping_triangles(&h_sh_white), (Some(Color32::WHITE), None));

        // Shadows: Red only.
        let h_sh_red = make_histogram([clip, 0, 0], [0, 0, 0], total);
        assert_eq!(clipping_triangles(&h_sh_red), (Some(Color32::from_rgb(255, 0, 0)), None));

        // Shadows: Green only.
        let h_sh_green = make_histogram([0, clip, 0], [0, 0, 0], total);
        assert_eq!(clipping_triangles(&h_sh_green), (Some(Color32::from_rgb(0, 255, 0)), None));

        // Shadows: Blue only.
        let h_sh_blue = make_histogram([0, 0, clip], [0, 0, 0], total);
        assert_eq!(clipping_triangles(&h_sh_blue), (Some(Color32::from_rgb(0, 0, 255)), None));

        // Shadows: Red + Green -> Yellow.
        let h_sh_yellow = make_histogram([clip, clip, 0], [0, 0, 0], total);
        assert_eq!(clipping_triangles(&h_sh_yellow), (Some(Color32::from_rgb(255, 255, 0)), None));

        // Shadows: Red + Blue -> Magenta.
        let h_sh_magenta = make_histogram([clip, 0, clip], [0, 0, 0], total);
        assert_eq!(clipping_triangles(&h_sh_magenta), (Some(Color32::from_rgb(255, 0, 255)), None));

        // Shadows: Green + Blue -> Cyan.
        let h_sh_cyan = make_histogram([0, clip, clip], [0, 0, 0], total);
        assert_eq!(clipping_triangles(&h_sh_cyan), (Some(Color32::from_rgb(0, 255, 255)), None));

        // Highlights: all three clip -> White.
        let h_hi_white = make_histogram([0, 0, 0], [clip, clip, clip], total);
        assert_eq!(clipping_triangles(&h_hi_white), (None, Some(Color32::WHITE)));

        // Highlights: Red + Green -> Yellow.
        let h_hi_yellow = make_histogram([0, 0, 0], [clip, clip, 0], total);
        assert_eq!(clipping_triangles(&h_hi_yellow), (None, Some(Color32::from_rgb(255, 255, 0))));

        // Highlights: Red + Blue -> Magenta.
        let h_hi_magenta = make_histogram([0, 0, 0], [clip, 0, clip], total);
        assert_eq!(clipping_triangles(&h_hi_magenta), (None, Some(Color32::from_rgb(255, 0, 255))));

        // Highlights: Green + Blue -> Cyan.
        let h_hi_cyan = make_histogram([0, 0, 0], [0, clip, clip], total);
        assert_eq!(clipping_triangles(&h_hi_cyan), (None, Some(Color32::from_rgb(0, 255, 255))));

        // Highlights: Red only.
        let h_hi_red = make_histogram([0, 0, 0], [clip, 0, 0], total);
        assert_eq!(clipping_triangles(&h_hi_red), (None, Some(Color32::from_rgb(255, 0, 0))));

        // Both shadows and highlights clipping simultaneously with different colors.
        let h_both = make_histogram([clip, 0, 0], [0, 0, clip], total);
        assert_eq!(
            clipping_triangles(&h_both),
            (Some(Color32::from_rgb(255, 0, 0)), Some(Color32::from_rgb(0, 0, 255)))
        );
    }

    #[test]
    fn empty_histogram_no_triangles() {
        let empty = Histogram {
            r: vec![0; 256],
            g: vec![0; 256],
            b: vec![0; 256],
            luma: vec![0; 256],
            total: 0,
            hdr: None,
        };
        assert_eq!(clipping_triangles(&empty), (None, None));
    }

    #[test]
    fn clipping_overlay_marks_blown_and_crushed_pixels() {
        let mut img = Rgba8::from_fn(3, 3, |x, y| match (x, y) {
            (0, 0) => [255, 255, 255, 255], // blown highlight (all 255)
            (1, 0) => [255, 50, 50, 255],   // blown highlight (red 255)
            (2, 0) => [50, 255, 50, 255],   // blown highlight (green 255)
            (0, 1) => [50, 50, 255, 255],   // blown highlight (blue 255)
            (1, 1) => [0, 0, 0, 255],       // crushed shadow (all 0)
            (2, 1) => [100, 150, 200, 255], // normal pixel
            (0, 2) => [0, 0, 10, 255],      // near-black (not all 0)
            (1, 2) => [254, 254, 254, 255], // near-white (no 255)
            (2, 2) => [0, 255, 255, 255],   // blown highlight (green and blue 255)
            _ => unreachable!(),
        });

        apply_clipping_overlay(&mut img);

        assert_eq!(img.get(0, 0), HIGHLIGHT);
        assert_eq!(img.get(1, 0), HIGHLIGHT);
        assert_eq!(img.get(2, 0), HIGHLIGHT);
        assert_eq!(img.get(0, 1), HIGHLIGHT);
        assert_eq!(img.get(1, 1), SHADOW);
        assert_eq!(img.get(2, 1), [100, 150, 200, 255]);
        assert_eq!(img.get(0, 2), [0, 0, 10, 255]);
        assert_eq!(img.get(1, 2), [254, 254, 254, 255]);
        assert_eq!(img.get(2, 2), HIGHLIGHT);

        // Test non-mutating copy function and overlay helper.
        let original = Rgba8::filled(2, 2, [0, 0, 0, 255]);
        let marked = clipping_overlay(original.clone());
        assert_eq!(marked.get(0, 0), SHADOW);
        assert_eq!(original.get(0, 0), [0, 0, 0, 255]);

        let via_overlay = overlay(&original);
        assert_eq!(via_overlay.get(0, 0), SHADOW);
    }

    #[test]
    fn widget_renders_without_panic() {
        let ctx = egui::Context::default();
        let theme = Theme::default();
        let img = Rgba8::from_fn(10, 10, |x, _| if x < 5 { [0, 0, 0, 255] } else { [255, 255, 255, 255] });
        let hist = Histogram::of_srgb8(&img);

        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(300.0, 400.0))),
            ..Default::default()
        };

        let mut clicked_none = true;
        let mut clicked_some = true;
        let mut output = ctx.run_ui(input, |ui| {
            clicked_none = show(ui, &theme, None, false);
            clicked_some = histogram(ui, &theme, Some(&hist), true);
        });
        output.textures_delta.clear();

        assert!(!clicked_none);
        assert!(!clicked_some);
    }

    #[test]
    fn clicking_triangle_returns_true() {
        let ctx = egui::Context::default();
        let theme = Theme::default();
        let img = Rgba8::from_fn(10, 10, |_, _| [0, 0, 0, 255]);
        let hist = Histogram::of_srgb8(&img);

        let input1 = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(300.0, 400.0))),
            ..Default::default()
        };
        let mut clicked = false;
        let mut out = ctx.run_ui(input1, |ui| {
            clicked = show(ui, &theme, Some(&hist), false);
        });
        out.textures_delta.clear();
        assert!(!clicked);

        let click_pos = pos2(14.0, 18.0);
        let input2 = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(300.0, 400.0))),
            events: vec![
                egui::Event::PointerMoved(click_pos),
                egui::Event::PointerButton {
                    pos: click_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: click_pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        };
        let mut out2 = ctx.run_ui(input2, |ui| {
            clicked = show(ui, &theme, Some(&hist), false);
        });
        out2.textures_delta.clear();
        assert!(clicked, "clicking the shadow triangle returns true");
    }
}
