//! M4's reference-set comparison: OmaLux side by side with Lightroom Classic
//! and Capture One on a reference set of frames, at fit and at 100%, and
//! measured against the camera's own JPEG.
//!
//! Modelled on `spike.rs` and `fringe.rs`.
//!
//! ```bash
//! cargo run --release -p omalux-engine --example quality -- path/to/reference-set --out /tmp/quality
//! cargo run --release -p omalux-engine --example quality -- path/to/reference-set --out /tmp/quality --at 0.3,0.6
//! ```

use std::path::{Path, PathBuf};

use lightcraft_codecs::{DecodeOptions, EncodeImage, decode, encode};
use lightcraft_pipeline::RenderRequest;
use lightcraft_raster::Rgba8;
use lightcraft_raster::resample::{Filter, resize};
use omalux_engine::compare::{self, Region};
use omalux_engine::look::{self, Profile};
use omalux_engine::{export, raw};

/// Sibling TIFF suffixes to look for beside each raw, in order.
const TIFF_SIBLINGS: [&str; 4] = [
    "lightroom-camera",
    "lightroom-adobe",
    "captureone",
    "darktable",
];

/// Parses `--at x,y` where `x` and `y` are shares of width and height in 0..=1.
pub fn parse_at(s: &str) -> Result<(f64, f64), String> {
    let (xs, ys) = s.split_once(',').ok_or_else(|| format!("invalid --at {s:?}: expected x,y"))?;
    let x: f64 = xs.trim().parse().map_err(|e| format!("invalid --at x in {s:?}: {e}"))?;
    let y: f64 = ys.trim().parse().map_err(|e| format!("invalid --at y in {s:?}: {e}"))?;
    if !x.is_finite() || !y.is_finite() || !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) {
        return Err(format!("invalid --at {s:?}: x and y must be between 0.0 and 1.0"));
    }
    Ok((x, y))
}

/// Decodes a TIFF file and converts it to display-encoded 8-bit sRGB with its embedded ICC profile.
pub fn decode_tiff(path: &Path) -> Result<Rgba8, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let decoded = decode(&bytes, DecodeOptions::default()).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(decoded.to_srgb8())
}

/// Combines `columns` side by side with a `gap` (in pixels) of black between them.
/// Tops are aligned (`y = 0`), and any area below a shorter column is black.
pub fn side_by_side(columns: &[Rgba8], gap: usize) -> Rgba8 {
    if columns.is_empty() {
        return Rgba8::new(0, 0);
    }
    let total_w = columns.iter().map(|c| c.width).sum::<usize>() + columns.len().saturating_sub(1) * gap;
    let max_h = columns.iter().map(|c| c.height).max().unwrap_or(0);
    if total_w == 0 || max_h == 0 {
        return Rgba8::new(0, 0);
    }
    let mut sheet = Rgba8::filled(total_w, max_h, [0, 0, 0, 255]);
    let mut x_offset = 0;
    for col in columns {
        for y in 0..col.height {
            let src_row = &col.data[y * col.width..(y + 1) * col.width];
            let dst_start = y * total_w + x_offset;
            sheet.data[dst_start..dst_start + col.width].copy_from_slice(src_row);
        }
        x_offset += col.width + gap;
    }
    sheet
}

/// Extracts a window of up to `target_w × target_h` at 100% from `img`, centred at
/// `(at.0, at.1)` (as shares 0..=1 of `img`'s width and height), moved as little as
/// keeps it inside `img`.
pub fn window_at(img: &Rgba8, at: (f64, f64), target_w: usize, target_h: usize) -> Rgba8 {
    let w = target_w.min(img.width);
    let h = target_h.min(img.height);
    if w == 0 || h == 0 {
        return Rgba8::new(0, 0);
    }
    let cx = at.0 * img.width as f64;
    let cy = at.1 * img.height as f64;
    let x0 = (cx - w as f64 / 2.0).round() as isize;
    let y0 = (cy - h as f64 / 2.0).round() as isize;
    let max_x = (img.width - w) as isize;
    let max_y = (img.height - h) as isize;
    let x = x0.clamp(0, max_x) as usize;
    let y = y0.clamp(0, max_y) as usize;
    img.crop(x, y, w, h)
}

/// Fits `img` into `max_edge` pixels on its long edge in linear light using a Box filter.
/// Preserves aspect ratio, and never upscales if both dimensions are already <= `max_edge`.
pub fn fit_to_box(img: &Rgba8, max_edge: usize) -> Rgba8 {
    let long = img.width.max(img.height);
    if long == 0 || long <= max_edge {
        return img.clone();
    }
    let s = (max_edge as f64 / long as f64).min(1.0);
    let w = ((img.width as f64 * s).round() as usize).max(1);
    let h = ((img.height as f64 * s).round() as usize).max(1);
    if w == img.width && h == img.height {
        img.clone()
    } else {
        resize(&img.to_linear(), w, h, Filter::Box).to_srgb8()
    }
}

/// Brings `img` to `(target_w, target_h)` in linear light using a Box filter.
/// Returns `img.clone()` if already of the target size.
pub fn bring_to_shape(img: &Rgba8, target_w: usize, target_h: usize) -> Rgba8 {
    let (target_w, target_h) = (target_w.max(1), target_h.max(1));
    if (img.width, img.height) == (target_w, target_h) {
        img.clone()
    } else {
        resize(&img.to_linear(), target_w, target_h, Filter::Box).to_srgb8()
    }
}

/// Encodes `image` as PNG and writes it to `path`.
fn png(path: &Path, image: &Rgba8) -> Result<(), String> {
    let bytes = encode::encode_png(&EncodeImage::rgba8(image), &Default::default()).map_err(|e| format!("{}: {e}", path.display()))?;
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Locates a sibling TIFF file with suffix `tag` (`.tif` or `.tiff`, any case) beside `stem` in `parent`.
fn find_sibling(parent: &Path, stem: &str, tag: &str) -> Option<PathBuf> {
    for ext in ["tif", "tiff", "TIF", "TIFF"] {
        let p = parent.join(format!("{stem}.{tag}.{ext}"));
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// Gathers raw files from `args`: files ending in `.arw` (case-insensitive), or searching directories.
fn raws(args: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for a in args {
        if a.is_dir() {
            let mut found: Vec<PathBuf> = std::fs::read_dir(a)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("arw")))
                .collect();
            found.sort();
            out.extend(found);
        } else {
            out.push(a.clone());
        }
    }
    out
}

/// Summary statistics collected across all frames.
#[derive(Default)]
struct Totals {
    frames: usize,
    /// Per column name, recorded mean ΔE2000 per region per frame: [All, Saturated, Skin, Sky].
    columns: std::collections::BTreeMap<String, Vec<[f64; 4]>>,
}

impl Totals {
    fn record(&mut self, col: &str, summaries: &[(Region, compare::Summary)]) {
        let mut row = [f64::NAN; 4];
        for (i, r) in Region::ALL.iter().enumerate() {
            if let Some((_, s)) = summaries.iter().find(|(reg, _)| reg == r)
                && s.share > 0.0
            {
                row[i] = s.mean;
            }
        }
        self.columns.entry(col.to_string()).or_default().push(row);
    }

    fn column_mean(&self, col: &str, region_idx: usize) -> Option<f64> {
        let rows = self.columns.get(col)?;
        let vals: Vec<f64> = rows.iter().map(|r| r[region_idx]).filter(|v| v.is_finite()).collect();
        if vals.is_empty() {
            None
        } else {
            Some(vals.iter().sum::<f64>() / vals.len() as f64)
        }
    }
}

/// Renders `bytes` with OmaLux as a fresh raw at full size in sRGB.
fn render_omalux(bytes: &[u8]) -> Result<Rgba8, String> {
    let loaded = raw::load(bytes, usize::MAX, Profile::Camera)?;
    let fresh = look::fresh(loaded.info.as_shot_temp, loaded.info.as_shot_tint);
    let (settings, info) = look::effective(&fresh, &loaded.info, &loaded.camera);
    let (w, h) = export::full_size(&loaded.image, &settings);
    let request = RenderRequest::fit(w, h);
    Ok(lightcraft_pipeline::render(&loaded.image, &info, &settings, &request).image)
}

fn process_frame(
    raw_path: &Path,
    out: Option<&Path>,
    ats: &[(f64, f64)],
    totals: &mut Totals,
) -> Result<(), String> {
    let name = raw_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let bytes = std::fs::read(raw_path).map_err(|e| format!("{}: {e}", raw_path.display()))?;

    let omalux_img = render_omalux(&bytes)?;
    let camera_img = raw::embedded_preview_srgb(&bytes, usize::MAX);

    let parent = raw_path.parent().unwrap_or(Path::new("."));
    let stem = raw_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();

    let mut columns: Vec<(&'static str, Rgba8)> = Vec::new();
    columns.push(("omalux", omalux_img));

    if let Some(ref cam) = camera_img {
        columns.push(("camera", cam.clone()));
    }

    for &tag in &TIFF_SIBLINGS {
        if let Some(tiff_path) = find_sibling(parent, &stem, tag) {
            match decode_tiff(&tiff_path) {
                Ok(img) => columns.push((tag, img)),
                Err(e) => println!("  {tag}: failed: {e}"),
            }
        }
    }

    let col_names: Vec<&str> = columns.iter().map(|(n, _)| *n).collect();
    println!("{name}: {}", col_names.join(", "));

    if let Some(ref camera) = camera_img {
        let (cw, ch) = (camera.width, camera.height);
        let (tw, th) = if cw >= ch {
            let th = ((ch as f64 * 1024.0 / cw as f64).round() as usize).max(1);
            (1024, th)
        } else {
            let tw = ((cw as f64 * 1024.0 / ch as f64).round() as usize).max(1);
            (tw, 1024)
        };
        let cam_1024 = bring_to_shape(camera, tw, th);

        for &(col_name, ref col_img) in &columns {
            if col_name == "camera" {
                continue;
            }
            let col_1024 = bring_to_shape(col_img, tw, th);
            match compare::compare(&col_1024, &cam_1024) {
                Ok(summaries) => {
                    let line: Vec<String> = summaries
                        .iter()
                        .filter(|(_, s)| s.share > 0.0)
                        .map(|(r, s)| format!("{} {:.2} / {:.2} ({:.0} %, L* {:+.1})", r.label(), s.mean, s.p95, s.share * 100.0, s.lightness))
                        .collect();
                    println!("  {col_name}: {}", line.join(", "));
                    totals.record(col_name, &summaries);
                }
                Err(e) => println!("  {col_name}: compare failed: {e}"),
            }
        }
    } else {
        println!("  no embedded camera JPEG to compare with");
    }

    if let Some(out) = out {
        let fit_cols: Vec<Rgba8> = columns.iter().map(|(_, img)| fit_to_box(img, 1000)).collect();
        let fit_sheet = side_by_side(&fit_cols, 8);
        png(&out.join(format!("{stem}-fit.png")), &fit_sheet)?;

        for (i, at) in ats.iter().enumerate() {
            let win_cols: Vec<Rgba8> = columns.iter().map(|(_, img)| window_at(img, *at, 800, 800)).collect();
            let win_sheet = side_by_side(&win_cols, 8);
            let filename = if ats.len() == 1 {
                format!("{stem}-100.png")
            } else {
                format!("{stem}-100-{}.png", i + 1)
            };
            png(&out.join(filename), &win_sheet)?;
        }
    }

    totals.frames += 1;
    Ok(())
}

fn main() {
    let mut raw_args = std::env::args().skip(1);
    let mut out_dir: Option<PathBuf> = None;
    let mut ats: Vec<(f64, f64)> = Vec::new();
    let mut paths: Vec<PathBuf> = Vec::new();

    while let Some(arg) = raw_args.next() {
        if arg == "--out" {
            let val = match raw_args.next() {
                Some(v) => v,
                None => {
                    eprintln!("error: --out requires a directory argument");
                    std::process::exit(2);
                }
            };
            out_dir = Some(PathBuf::from(val));
        } else if let Some(stripped) = arg.strip_prefix("--out=") {
            out_dir = Some(PathBuf::from(stripped));
        } else if arg == "--at" {
            let val = match raw_args.next() {
                Some(v) => v,
                None => {
                    eprintln!("error: --at requires an x,y argument");
                    std::process::exit(2);
                }
            };
            match parse_at(&val) {
                Ok(at) => ats.push(at),
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(2);
                }
            }
        } else if let Some(stripped) = arg.strip_prefix("--at=") {
            match parse_at(stripped) {
                Ok(at) => ats.push(at),
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(2);
                }
            }
        } else if arg.starts_with('-') {
            eprintln!("unknown option: {arg}");
            std::process::exit(2);
        } else {
            paths.push(PathBuf::from(arg));
        }
    }

    if ats.is_empty() {
        ats.push((0.5, 0.5));
    }

    if let Some(ref out) = out_dir
        && let Err(e) = std::fs::create_dir_all(out)
    {
        eprintln!("{}: {e}", out.display());
        std::process::exit(1);
    }

    let files = raws(&paths);
    if files.is_empty() {
        eprintln!("usage: quality <reference set folder or raws…> [--out DIR] [--at X,Y]");
        std::process::exit(2);
    }

    let mut totals = Totals::default();
    for file in &files {
        if let Err(e) = process_frame(file, out_dir.as_deref(), &ats, &mut totals) {
            println!("{}: failed: {e}", file.display());
        }
    }

    if totals.frames == 0 {
        return;
    }

    println!();
    let known_cols = ["omalux", "lightroom-camera", "lightroom-adobe", "captureone", "darktable"];
    for col in known_cols {
        if totals.columns.contains_key(col) {
            let parts: Vec<String> = Region::ALL
                .iter()
                .enumerate()
                .filter_map(|(i, r)| totals.column_mean(col, i).map(|m| format!("{} {:.2}", r.label(), m)))
                .collect();
            if !parts.is_empty() {
                println!("{col}: {}", parts.join(", "));
            }
        }
    }

    let oma_overall = totals.column_mean("omalux", 0);
    let lr_overall = totals.column_mean("lightroom-camera", 0);
    match (oma_overall, lr_overall) {
        (Some(oma), Some(lr)) => {
            if oma <= lr {
                println!("OmaLux is at least as close to the camera as lightroom-camera overall ({oma:.2} vs {lr:.2})");
            } else {
                println!("OmaLux is not as close to the camera as lightroom-camera overall ({oma:.2} vs {lr:.2})");
            }
        }
        _ => {
            println!("no lightroom-camera render to compare with");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_by_side_sheet_size_and_column_placement_with_different_heights() {
        let col_a = Rgba8::filled(10, 20, [255, 0, 0, 255]);
        let col_b = Rgba8::filled(15, 30, [0, 255, 0, 255]);
        let col_c = Rgba8::filled(8, 12, [0, 0, 255, 255]);
        let gap = 8;
        let sheet = side_by_side(&[col_a, col_b, col_c], gap);

        assert_eq!(sheet.width, 10 + gap + 15 + gap + 8);
        assert_eq!(sheet.height, 30);

        // Column A: x in 0..10. Rows 0..20 are red, rows 20..30 are black.
        for y in 0..20 {
            for x in 0..10 {
                assert_eq!(sheet.get(x, y), [255, 0, 0, 255], "col_a pixel at ({x}, {y})");
            }
        }
        for y in 20..30 {
            for x in 0..10 {
                assert_eq!(sheet.get(x, y), [0, 0, 0, 255], "col_a shorter space at ({x}, {y})");
            }
        }

        // Gap 1: x in 10..18. All rows are black.
        for y in 0..30 {
            for x in 10..18 {
                assert_eq!(sheet.get(x, y), [0, 0, 0, 255], "gap 1 pixel at ({x}, {y})");
            }
        }

        // Column B: x in 18..33. All rows are green.
        for y in 0..30 {
            for x in 18..33 {
                assert_eq!(sheet.get(x, y), [0, 255, 0, 255], "col_b pixel at ({x}, {y})");
            }
        }

        // Gap 2: x in 33..41. All rows are black.
        for y in 0..30 {
            for x in 33..41 {
                assert_eq!(sheet.get(x, y), [0, 0, 0, 255], "gap 2 pixel at ({x}, {y})");
            }
        }

        // Column C: x in 41..49. Rows 0..12 are blue, rows 12..30 are black.
        for y in 0..12 {
            for x in 41..49 {
                assert_eq!(sheet.get(x, y), [0, 0, 255, 255], "col_c pixel at ({x}, {y})");
            }
        }
        for y in 12..30 {
            for x in 41..49 {
                assert_eq!(sheet.get(x, y), [0, 0, 0, 255], "col_c shorter space at ({x}, {y})");
            }
        }
    }

    #[test]
    fn window_stays_inside_for_corners_and_smaller_images() {
        let img = Rgba8::from_fn(1000, 900, |x, y| [(x % 256) as u8, (y % 256) as u8, 128, 255]);

        // Centred at corners: window must be exactly 800×800 and within bounds.
        for at in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
            let win = window_at(&img, at, 800, 800);
            assert_eq!(win.width, 800);
            assert_eq!(win.height, 800);
        }

        let tl = window_at(&img, (0.0, 0.0), 800, 800);
        assert_eq!(tl.get(0, 0), img.get(0, 0));
        assert_eq!(tl.get(799, 799), img.get(799, 799));

        let br = window_at(&img, (1.0, 1.0), 800, 800);
        assert_eq!(br.get(0, 0), img.get(200, 100));
        assert_eq!(br.get(799, 799), img.get(999, 899));

        // Image smaller than window: stays inside, size equals image size.
        let small = Rgba8::from_fn(400, 300, |x, y| [(x % 256) as u8, (y % 256) as u8, 50, 255]);
        for at in [(0.5, 0.5), (0.0, 0.0), (1.0, 1.0)] {
            let win = window_at(&small, at, 800, 800);
            assert_eq!(win.width, 400);
            assert_eq!(win.height, 300);
            assert_eq!(win.data, small.data);
        }
    }

    #[test]
    fn linear_prophoto_tiff_decodes_to_srgb_within_two_levels() {
        let colours = [
            [255u8, 255u8, 255u8],
            [128u8, 128u8, 128u8],
            [180u8, 120u8, 60u8],
            [40u8, 80u8, 160u8],
        ];

        for expected in colours {
            let lin_srgb = expected.map(|c| lightcraft_color::transfer::srgb_to_linear(f32::from(c) / 255.0));
            let m = lightcraft_color::SRGB.to_space(&lightcraft_color::PROPHOTO);
            let pro = m.apply_f32(lin_srgb);
            let samples: Vec<u16> = pro.map(|v| (v.clamp(0.0, 1.0) * 65535.0).round() as u16).to_vec();

            let linear_img = export::Linear16 { width: 1, height: 1, samples };
            let tiff_bytes = export::tiff(&linear_img).expect("tiff encode failed");

            let decoded = decode(&tiff_bytes, DecodeOptions::default()).expect("tiff decode failed");
            let srgb8 = decoded.to_srgb8();

            assert_eq!(srgb8.width, 1);
            assert_eq!(srgb8.height, 1);
            let p = srgb8.data[0];

            for c in 0..3 {
                let diff = p[c].abs_diff(expected[c]);
                assert!(diff <= 2, "colour {expected:?}, channel {c}: got {}, diff {diff}", p[c]);
            }
        }
    }

    #[test]
    fn at_parsing() {
        // Good cases
        assert_eq!(parse_at("0.3,0.6").expect("parse_at 0.3,0.6"), (0.3, 0.6));
        assert_eq!(parse_at("0.5,0.5").expect("parse_at 0.5,0.5"), (0.5, 0.5));
        assert_eq!(parse_at("0,1").expect("parse_at 0,1"), (0.0, 1.0));
        assert_eq!(parse_at(" 0.2 , 0.8 ").expect("parse_at whitespace"), (0.2, 0.8));

        // Bad format
        assert!(parse_at("").is_err());
        assert!(parse_at("0.5").is_err());
        assert!(parse_at("foo,bar").is_err());
        assert!(parse_at("0.5,foo").is_err());
        assert!(parse_at("0.1,0.2,0.3").is_err());

        // Out of 0..1
        assert!(parse_at("-0.1,0.5").is_err());
        assert!(parse_at("0.5,-0.01").is_err());
        assert!(parse_at("1.01,0.5").is_err());
        assert!(parse_at("0.5,1.5").is_err());
    }

    #[test]
    fn trial_run_with_corpus() {
        let corpus_dir = match std::env::var_os("LIGHTCRAFT_CORPUS") {
            Some(p) => PathBuf::from(p).join("raw"),
            None => return,
        };
        let raw1 = corpus_dir.join("arw-sony-a7m3-compressed.arw");
        let raw2 = corpus_dir.join("arw-sony-a7m3-uncompressed.arw");
        if !raw1.exists() || !raw2.exists() {
            return;
        }

        let temp = std::env::temp_dir().join(format!("omalux-quality-trial-{}", std::process::id()));
        let out_dir = temp.join("out");
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).expect("create temp dir");
        std::fs::create_dir_all(&out_dir).expect("create out dir");

        for raw_src in [&raw1, &raw2] {
            let file_name = raw_src.file_name().expect("file name");
            let raw_dest = temp.join(file_name);
            std::fs::copy(raw_src, &raw_dest).expect("copy raw");
            assert!(matches!(export::export(&raw_dest, &temp, None, None).expect("export raw"), export::Exported::Written { .. }));
            let stem = raw_dest.file_stem().expect("stem").to_string_lossy();
            let generated_tif = temp.join(format!("{stem}.tif"));
            let lr_tif = temp.join(format!("{stem}.lightroom-camera.tif"));
            std::fs::rename(generated_tif, lr_tif).expect("rename tif");
        }

        let mut totals = Totals::default();
        let ats = [(0.5, 0.5)];
        for raw_src in [&raw1, &raw2] {
            let file_name = raw_src.file_name().expect("file name");
            let raw_dest = temp.join(file_name);
            process_frame(&raw_dest, Some(&out_dir), &ats, &mut totals).expect("process frame");
        }

        let oma_all = totals.column_mean("omalux", 0).expect("oma overall");
        let lr_all = totals.column_mean("lightroom-camera", 0).expect("lr overall");
        assert!((oma_all - lr_all).abs() < 0.05, "oma: {oma_all}, lr: {lr_all}");

        assert!(out_dir.join("arw-sony-a7m3-compressed-fit.png").exists());
        assert!(out_dir.join("arw-sony-a7m3-compressed-100.png").exists());
        assert!(out_dir.join("arw-sony-a7m3-uncompressed-fit.png").exists());
        assert!(out_dir.join("arw-sony-a7m3-uncompressed-100.png").exists());

        let _ = std::fs::remove_dir_all(&temp);
    }
}
