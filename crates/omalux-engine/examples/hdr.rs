//! M4's check of the HDR slider's local tone mapping against exposure
//! fusion on real frames (see "HDR" in docs/DESIGN.md and M4 in docs/ROADMAP.md).
//!
//! Four renders of each frame at 1600 px on the long edge: HDR 0, HDR 50,
//! HDR 100 and exposure fusion (-2, -1, 0, +1, +2 EV). With `--out`, writes
//! `NAME-hdr.png` with the four side by side separated by an 8 px black gap.
//!
//! ```bash
//! cargo run --release -p omalux-engine --example hdr -- path/to/raws-or-a-folder --out /tmp/hdr
//! ```

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use lightcraft_pipeline::RenderRequest;
use lightcraft_raster::{Rgb32f, Rgba8};
use omalux_engine::compare;
use omalux_engine::fusion;
use omalux_engine::look::{self, Profile};
use omalux_engine::raw;

/// The long edge of the four comparison renders.
const RENDER_EDGE: usize = 1600;
/// The source preview edge for decoding the raw.
const PREVIEW_EDGE: usize = 2560;
/// The gap between frames in the side-by-side sheet.
const GAP: usize = 8;
/// Exposure offsets in EV for fusion.
const FUSION_OFFSETS: [f64; 5] = [-2.0, -1.0, 0.0, 1.0, 2.0];
const LABELS: [&str; 4] = ["HDR   0", "HDR  50", "HDR 100", "fusion "];

#[derive(Clone, Copy, Debug, Default)]
struct RenderStats {
    share_255: f64,
    share_0: f64,
    mean_l_dark: f64,
    mean_l_bright: f64,
}

impl RenderStats {
    fn add(&mut self, other: &RenderStats) {
        self.share_255 += other.share_255;
        self.share_0 += other.share_0;
        self.mean_l_dark += other.mean_l_dark;
        self.mean_l_bright += other.mean_l_bright;
    }

    fn scale(&self, factor: f64) -> RenderStats {
        RenderStats {
            share_255: self.share_255 * factor,
            share_0: self.share_0 * factor,
            mean_l_dark: self.mean_l_dark * factor,
            mean_l_bright: self.mean_l_bright * factor,
        }
    }
}

fn stats(r: &Rgba8, dark_indices: &[usize], bright_indices: &[usize]) -> RenderStats {
    let n = r.data.len();
    let count_255 = r.data.iter().filter(|p| p[0] == 255 || p[1] == 255 || p[2] == 255).count();
    let count_0 = r.data.iter().filter(|p| p[0] == 0 || p[1] == 0 || p[2] == 0).count();
    let q_dark = dark_indices.len().max(1) as f64;
    let q_bright = bright_indices.len().max(1) as f64;
    let mean_l_dark = dark_indices.iter().map(|&i| compare::lab([r.data[i][0], r.data[i][1], r.data[i][2]])[0]).sum::<f64>() / q_dark;
    let mean_l_bright = bright_indices.iter().map(|&i| compare::lab([r.data[i][0], r.data[i][1], r.data[i][2]])[0]).sum::<f64>() / q_bright;
    RenderStats {
        share_255: count_255 as f64 / n.max(1) as f64,
        share_0: count_0 as f64 / n.max(1) as f64,
        mean_l_dark,
        mean_l_bright,
    }
}

fn format_stats(label: &str, s: &RenderStats) -> String {
    format!(
        "  {label}: clipped 255 {:5.2} %, clipped 0 {:5.2} %, dark quarter L* {:5.1}, bright quarter L* {:5.1}",
        s.share_255 * 100.0,
        s.share_0 * 100.0,
        s.mean_l_dark,
        s.mean_l_bright
    )
}

fn png(path: &Path, image: &Rgba8) -> Result<(), String> {
    let bytes = lightcraft_codecs::encode::encode_png(&lightcraft_codecs::EncodeImage::rgba8(image), &Default::default()).map_err(|e| e.to_string())?;
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn check(file: &Path, out: Option<&Path>) -> Result<[RenderStats; 4], String> {
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let bytes = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let loaded = raw::load(&bytes, PREVIEW_EDGE, Profile::Camera, omalux_engine::sidecar::PROCESS_VERSION)?;
    let src = Arc::new(loaded.image);
    let info = loaded.info;
    let camera = loaded.camera;

    let fresh = look::fresh(info.as_shot_temp, info.as_shot_tint);
    let req = RenderRequest::fit(RENDER_EDGE, RENDER_EDGE);

    // 1. HDR 0, 2. HDR 50, 3. HDR 100
    let mut s1 = fresh.clone();
    s1.light.hdr = 0.0;
    let mut s2 = fresh.clone();
    s2.light.hdr = 50.0;
    let mut s3 = fresh.clone();
    s3.light.hdr = 100.0;

    let t_sliders_start = Instant::now();
    let (eff_s1, eff_i1) = look::effective(&s1, &info, &camera);
    let r1 = lightcraft_pipeline::render(&src, &eff_i1, &eff_s1, &req).image;
    let (eff_s2, eff_i2) = look::effective(&s2, &info, &camera);
    let r2 = lightcraft_pipeline::render(&src, &eff_i2, &eff_s2, &req).image;
    let (eff_s3, eff_i3) = look::effective(&s3, &info, &camera);
    let r3 = lightcraft_pipeline::render(&src, &eff_i3, &eff_s3, &req).image;
    let t_sliders = t_sliders_start.elapsed().as_secs_f64() * 1e3;

    // 4. Fusion: fresh settings with light.exposure at -2, -1, 0, +1, +2 (HDR 0)
    let t_fusion_start = Instant::now();
    let mut exposures = Vec::with_capacity(FUSION_OFFSETS.len());
    for ev in FUSION_OFFSETS {
        let mut exp_settings = fresh.clone();
        exp_settings.light.hdr = 0.0;
        exp_settings.light.exposure += ev;
        let (eff_s, eff_i) = look::effective(&exp_settings, &info, &camera);
        let rendered = lightcraft_pipeline::render(&src, &eff_i, &eff_s, &req).image;
        let exp_f32 = Rgb32f {
            width: rendered.width,
            height: rendered.height,
            data: rendered
                .data
                .iter()
                .map(|p| [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0])
                .collect(),
        };
        exposures.push(exp_f32);
    }
    let t_fusion_renders = t_fusion_start.elapsed().as_secs_f64() * 1e3;

    let t_fuse_start = Instant::now();
    let fused = fusion::fuse(&exposures).ok_or_else(|| format!("{}: fusion failed", file.display()))?;
    let c8 = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
    let r4 = Rgba8 {
        width: fused.width,
        height: fused.height,
        data: fused.data.iter().map(|p| [c8(p[0]), c8(p[1]), c8(p[2]), 255]).collect(),
    };
    let t_fuse = t_fuse_start.elapsed().as_secs_f64() * 1e3;
    let t_fusion_total = t_fusion_renders + t_fuse;

    println!("{name}:");
    println!("  renders: 3 sliders in {t_sliders:.0} ms (HDR 0, 50, 100); fusion in {t_fusion_total:.0} ms (5 renders {t_fusion_renders:.0} ms, fuse {t_fuse:.0} ms)");

    // Darkest and brightest quarters in render 1 (by L*)
    let n = r1.data.len();
    let q = n / 4;
    let l1: Vec<f64> = r1.data.iter().map(|p| compare::lab([p[0], p[1], p[2]])[0]).collect();
    let mut indices: Vec<usize> = (0..n).collect();
    indices.sort_by(|&a, &b| l1[a].total_cmp(&l1[b]));
    let dark_indices = &indices[..q];
    let bright_indices = &indices[n - q..];

    let renders = [&r1, &r2, &r3, &r4];
    let per_render_stats = std::array::from_fn(|i| stats(renders[i], dark_indices, bright_indices));
    for (i, s) in per_render_stats.iter().enumerate() {
        println!("{}", format_stats(LABELS[i], s));
    }

    if let Some(out) = out {
        let (w, h) = (r1.width, r1.height);
        let sheet_w = w * 4 + GAP * 3;
        let mut sheet = Rgba8::filled(sheet_w, h, [0, 0, 0, 255]);
        for (i, r) in renders.iter().enumerate() {
            let x_offset = i * (w + GAP);
            for y in 0..h.min(r.height) {
                let row_len = w.min(r.width);
                let src_row = &r.data[y * r.width..y * r.width + row_len];
                let dst_row = &mut sheet.data[y * sheet_w + x_offset..y * sheet_w + x_offset + row_len];
                dst_row.copy_from_slice(src_row);
            }
        }
        let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("frame");
        let path = out.join(format!("{stem}-hdr.png"));
        png(&path, &sheet)?;
        println!("  wrote {}", path.display());
    }

    Ok(per_render_stats)
}

fn main() {
    let mut args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let out = args.iter().position(|a| a == Path::new("--out")).map(|i| {
        args.remove(i);
        args.remove(i)
    });
    if let Some(out) = &out
        && let Err(e) = std::fs::create_dir_all(out)
    {
        eprintln!("{}: {e}", out.display());
        std::process::exit(1);
    }
    let mut files = Vec::new();
    for a in &args {
        match std::fs::read_dir(a) {
            Ok(dir) => {
                let mut found: Vec<PathBuf> = dir
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("arw")))
                    .collect();
                found.sort();
                files.extend(found);
            }
            Err(_) => files.push(a.clone()),
        }
    }
    if files.is_empty() {
        eprintln!("usage: hdr <folder or raws…> [--out DIR]");
        std::process::exit(2);
    }

    let mut cumulative: [RenderStats; 4] = Default::default();
    let mut succeeded = 0;

    for file in &files {
        match check(file, out.as_deref()) {
            Ok(stats) => {
                for i in 0..4 {
                    cumulative[i].add(&stats[i]);
                }
                succeeded += 1;
            }
            Err(e) => {
                eprintln!("{}: failed: {e}", file.display());
            }
        }
    }

    if succeeded > 0 {
        let n = succeeded as f64;
        let suffix = if succeeded == 1 { "frame" } else { "frames" };
        println!("Average over {succeeded} {suffix}:");
        for i in 0..4 {
            let avg = cumulative[i].scale(1.0 / n);
            println!("{}", format_stats(LABELS[i], &avg));
        }
    }
}
