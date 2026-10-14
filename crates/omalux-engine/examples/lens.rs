//! M5's lens comparison against the camera's JPEG: measures distortion and
//! vignetting for each lens profile source (none, lensfun, Sony, Lightroom)
//! against the camera's own corrected embedded preview.
//!
//!     cargo run --release -p omalux-engine --example lens -- path/to/shoot --out /tmp/lens

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lightcraft_codecs::{DecodeOptions, EncodeImage, decode, encode};
use lightcraft_pipeline::RenderRequest;
use lightcraft_raster::Rgba8;
use lightcraft_raster::resample::{Filter, resize};
use lightcraft_tiff::{Tiff, Value, makernote, tags};
use omalux_engine::look::{self, Profile};
use omalux_engine::{export, lens_check, lens_profiles, raw};

const SONY_TAG9402: u16 = 0x9402;

const DECIPHER: [u8; 256] = {
    let mut inv = [0u8; 256];
    let mut p = 0usize;
    while p < 256 {
        let c = if p < 249 { p * p % 249 * p % 249 } else { p };
        inv[c] = p as u8;
        p += 1;
    }
    inv
};

/// Reads focus distance (in metres) from Sony MakerNote Tag 0x9402 FocusPosition2.
fn sony_focus_distance(bytes: &[u8], focal_length: f64) -> Option<f64> {
    let tiff = Tiff::parse(bytes).ok()?;
    let make = tiff.ifds.first()?.string(tags::MAKE)?;
    if !make.trim().to_ascii_uppercase().starts_with("SONY") {
        return None;
    }
    let entry = tiff.exif()?.get(tags::MAKER_NOTE)?;
    let note = makernote::parse_makernote(bytes, entry.offset, entry.count() as u64, tiff.order, &make)?;
    let entry_9402 = note.ifd.get(SONY_TAG9402)?;

    let block = match &entry_9402.value {
        Value::Undefined(v) if v.len() >= 0x2e => v.as_slice(),
        _ => {
            let start = entry_9402.offset as usize;
            bytes.get(start..start + 400)?
        }
    };

    let deciphered_2d = DECIPHER[block[0x2d] as usize];
    if deciphered_2d == 255 {
        Some(f64::INFINITY)
    } else {
        Some((2.0f64.powf(deciphered_2d as f64 / 16.0 - 5.0) + 1.0) * focal_length / 1000.0)
    }
}

/// Formats focus distance for display.
fn format_distance(dist: Option<f64>) -> String {
    match dist {
        Some(d) if d.is_infinite() => "inf".into(),
        Some(d) => format!("{d:.1} m"),
        None => "unknown".into(),
    }
}

/// Locates a sibling TIFF file with suffix `tag` (.tif or .tiff) beside `stem` in `parent`.
fn find_sibling(parent: &Path, stem: &str, tag: &str) -> Option<PathBuf> {
    for ext in ["tif", "tiff", "TIF", "TIFF"] {
        let p = parent.join(format!("{stem}.{tag}.{ext}"));
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// Decodes a TIFF file and converts it to display-encoded 8-bit sRGB with its embedded ICC profile.
fn decode_tiff(path: &Path) -> Result<Rgba8, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let decoded = decode(&bytes, DecodeOptions::default()).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(decoded.to_srgb8())
}

/// Combines columns side by side with a black gap of `gap` pixels between them.
fn side_by_side(columns: &[Rgba8], gap: usize) -> Rgba8 {
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

/// Fits `img` into `max_edge` pixels on its long edge in linear light using a Box filter.
fn fit_to_box(img: &Rgba8, max_edge: usize) -> Rgba8 {
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
fn bring_to_shape(img: &Rgba8, target_w: usize, target_h: usize) -> Rgba8 {
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

/// Extracts all raw files from `args`.
fn gather_raws(args: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
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
            files.extend(found);
        } else {
            files.push(a.clone());
        }
    }
    files
}

/// Records summary statistics for each lens and aperture group across frames.
#[derive(Default)]
struct SummaryTracker {
    groups: BTreeMap<(String, String), GroupStats>,
}

#[derive(Default)]
struct GroupStats {
    count: usize,
    sources: BTreeMap<String, SourceStats>,
}

#[derive(Default)]
struct SourceStats {
    residuals: Vec<f64>,
    vignettings: Vec<f64>,
}

impl SummaryTracker {
    fn record(&mut self, lens: &str, aperture: &str, source: &str, m: &lens_check::Measure) {
        let key = (lens.to_string(), aperture.to_string());
        let group = self.groups.entry(key).or_default();
        let src = group.sources.entry(source.to_string()).or_default();
        src.residuals.push(m.median_residual);
        src.vignettings.push(m.vignetting_diff);
    }

    fn increment_frame(&mut self, lens: &str, aperture: &str) {
        let key = (lens.to_string(), aperture.to_string());
        self.groups.entry(key).or_default().count += 1;
    }

    fn print(&self) {
        println!("Summary per lens and aperture:");
        for ((lens, aperture), group) in &self.groups {
            println!("{lens} at {aperture} ({} frame{}):", group.count, if group.count == 1 { "" } else { "s" });
            for (source, stats) in &group.sources {
                if stats.residuals.is_empty() {
                    continue;
                }
                let mut sorted_res = stats.residuals.clone();
                sorted_res.sort_by(f64::total_cmp);
                let med_res = sorted_res[sorted_res.len() / 2];
                let mean_vig = stats.vignettings.iter().sum::<f64>() / stats.vignettings.len() as f64;
                println!("  {source:<9} residual med {med_res:.2} px, vignetting {mean_vig:+.1} L*");
            }
        }
    }
}

fn process_raw(
    raw_path: &Path,
    out: Option<&Path>,
    summary: &mut SummaryTracker,
) -> Result<(), String> {
    let name = raw_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let bytes = std::fs::read(raw_path).map_err(|e| format!("{}: {e}", raw_path.display()))?;
    let meta = lightcraft_meta::extract(&bytes);

    let lens_name = meta.lens_model.clone().unwrap_or_else(|| "no lens named".into());
    let focal = meta.focal_length.unwrap_or(0.0);
    let aperture = meta.f_number.unwrap_or(0.0);
    let distance = sony_focus_distance(&bytes, focal);
    let ap_label = format!("f/{aperture:.1}");

    println!("{name}: {lens_name} at {focal:.0} mm {ap_label}, focus {}", format_distance(distance));

    let camera_img = raw::embedded_preview_srgb(&bytes, usize::MAX).ok_or_else(|| "no embedded camera JPEG".to_string())?;
    let (cw, ch) = (camera_img.width, camera_img.height);

    let raw_image = lightcraft_raw::decode(&bytes).map_err(|e| format!("{name}: {e}"))?;
    let raw_info = raw_image.info();
    let loaded = raw::load(&bytes, usize::MAX, Profile::Camera)?;

    let lensfun_found = lens_profiles::for_raw(&raw_info, &bytes, lens_profiles::Source::Lensfun)
        .filter(|f| f.source == lens_profiles::Source::Lensfun);
    let sony_found = lens_profiles::for_raw(&raw_info, &bytes, lens_profiles::Source::Sony)
        .filter(|f| f.source == lens_profiles::Source::Sony);

    let mut found_str = Vec::new();
    if lensfun_found.is_some() {
        found_str.push("lensfun");
    }
    if sony_found.is_some() {
        found_str.push("Sony");
    }
    if found_str.is_empty() {
        println!("  sources found: none");
    } else {
        println!("  sources found: {}", found_str.join(", "));
    }

    let request_cam = RenderRequest::fit(cw, ch);

    // 1. None (uncorrected)
    let fresh = look::fresh(loaded.info.as_shot_temp, loaded.info.as_shot_tint);
    let mut none_info = loaded.info.clone();
    none_info.lens = None;
    let (none_settings, none_eff_info) = look::effective(&fresh, &none_info, &loaded.camera);
    let render_none = lightcraft_pipeline::render(&loaded.image, &none_eff_info, &none_settings, &request_cam).image;
    let (none_settings, none_eff_info) = (none_settings.into_owned(), none_eff_info.into_owned());

    // 2. Lensfun
    let render_lensfun = lensfun_found.as_ref().map(|found| {
        let mut s = look::fresh(loaded.info.as_shot_temp, loaded.info.as_shot_tint);
        s.optics.lens_profile = true;
        let mut cur_info = loaded.info.clone();
        cur_info.lens = Some(found.lens);
        let (eff_settings, eff_info) = look::effective(&s, &cur_info, &loaded.camera);
        let img = lightcraft_pipeline::render(&loaded.image, &eff_info, &eff_settings, &request_cam).image;
        (img, eff_settings.into_owned(), eff_info.into_owned())
    });

    // 3. Sony
    let render_sony = sony_found.as_ref().map(|found| {
        let mut s = look::fresh(loaded.info.as_shot_temp, loaded.info.as_shot_tint);
        s.optics.lens_profile = true;
        let mut cur_info = loaded.info.clone();
        cur_info.lens = Some(found.lens);
        let (eff_settings, eff_info) = look::effective(&s, &cur_info, &loaded.camera);
        let img = lightcraft_pipeline::render(&loaded.image, &eff_info, &eff_settings, &request_cam).image;
        (img, eff_settings.into_owned(), eff_info.into_owned())
    });

    // 4. Sibling Lightroom TIFF if present
    let parent = raw_path.parent().unwrap_or(Path::new("."));
    let stem = raw_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let lr_tiff = find_sibling(parent, &stem, "lightroom-lens").and_then(|p| decode_tiff(&p).ok());

    // Measure each source against the camera JPEG
    let m_none = lens_check::check(&render_none, &camera_img)?;
    let none_res_bins = m_none.bins.iter().map(|b| format!("{:.2}", b.mean_residual)).collect::<Vec<_>>().join(", ");
    let none_l_bins = m_none.bins.iter().map(|b| format!("{:+.1}", b.delta_l)).collect::<Vec<_>>().join(", ");
    println!(
        "  none:     residual med {:.2} px, p95 {:.2} px (bins: {none_res_bins} px), vignetting {:+.1} L* (L* bins: {none_l_bins}, scale {:.3}, {} patches)",
        m_none.median_residual, m_none.p95_residual, m_none.vignetting_diff, m_none.scale, m_none.patches_used
    );
    summary.record(&lens_name, &ap_label, "none", &m_none);

    if let Some((ref r_lensfun, _, _)) = render_lensfun {
        let m = lens_check::check(r_lensfun, &camera_img)?;
        let lf_res_bins = m.bins.iter().map(|b| format!("{:.2}", b.mean_residual)).collect::<Vec<_>>().join(", ");
        let lf_l_bins = m.bins.iter().map(|b| format!("{:+.1}", b.delta_l)).collect::<Vec<_>>().join(", ");
        println!(
            "  lensfun:  residual med {:.2} px, p95 {:.2} px (bins: {lf_res_bins} px), vignetting {:+.1} L* (L* bins: {lf_l_bins}, scale {:.3}, {} patches)",
            m.median_residual, m.p95_residual, m.vignetting_diff, m.scale, m.patches_used
        );
        summary.record(&lens_name, &ap_label, "lensfun", &m);
    }

    if let Some((ref r_sony, _, _)) = render_sony {
        let m = lens_check::check(r_sony, &camera_img)?;
        let sy_res_bins = m.bins.iter().map(|b| format!("{:.2}", b.mean_residual)).collect::<Vec<_>>().join(", ");
        let sy_l_bins = m.bins.iter().map(|b| format!("{:+.1}", b.delta_l)).collect::<Vec<_>>().join(", ");
        println!(
            "  Sony:     residual med {:.2} px, p95 {:.2} px (bins: {sy_res_bins} px), vignetting {:+.1} L* (L* bins: {sy_l_bins}, scale {:.3}, {} patches)",
            m.median_residual, m.p95_residual, m.vignetting_diff, m.scale, m.patches_used
        );
        summary.record(&lens_name, &ap_label, "Sony", &m);
    } else {
        println!("  Sony:     not found (stub)");
    }

    if let Some(ref lr) = lr_tiff {
        let lr_matched = bring_to_shape(lr, cw, ch);
        let m = lens_check::check(&lr_matched, &camera_img)?;
        let lr_res_bins = m.bins.iter().map(|b| format!("{:.2}", b.mean_residual)).collect::<Vec<_>>().join(", ");
        let lr_l_bins = m.bins.iter().map(|b| format!("{:+.1}", b.delta_l)).collect::<Vec<_>>().join(", ");
        println!(
            "  lightroom: residual med {:.2} px, p95 {:.2} px (bins: {lr_res_bins} px), vignetting {:+.1} L* (L* bins: {lr_l_bins}, scale {:.3}, {} patches)",
            m.median_residual, m.p95_residual, m.vignetting_diff, m.scale, m.patches_used
        );
        summary.record(&lens_name, &ap_label, "lightroom", &m);
    }

    summary.increment_frame(&lens_name, &ap_label);

    // If --out specified, write fit sheet and 100% corner sheet
    if let Some(out_dir) = out {
        let mut fit_cols = Vec::new();
        fit_cols.push(fit_to_box(&render_none, 1000));
        if let Some((ref r_lf, _, _)) = render_lensfun {
            fit_cols.push(fit_to_box(r_lf, 1000));
        }
        if let Some((ref r_sy, _, _)) = render_sony {
            fit_cols.push(fit_to_box(r_sy, 1000));
        }
        if let Some(ref lr) = lr_tiff {
            fit_cols.push(fit_to_box(lr, 1000));
        }
        fit_cols.push(fit_to_box(&camera_img, 1000));

        let fit_sheet = side_by_side(&fit_cols, 8);
        png(&out_dir.join(format!("{stem}-fit.png")), &fit_sheet)?;

        // Top-left corner 600 px square from full-size renders
        let mut corner_cols = Vec::new();

        let (full_w, full_h) = export::full_size(&loaded.image, &none_settings);
        let full_req = RenderRequest::fit(full_w, full_h);
        let full_none = lightcraft_pipeline::render(&loaded.image, &none_eff_info, &none_settings, &full_req).image;
        corner_cols.push(full_none.crop(0, 0, 600.min(full_w), 600.min(full_h)));

        if let Some((_, ref lf_settings, ref lf_info)) = render_lensfun {
            let (w, h) = export::full_size(&loaded.image, lf_settings);
            let req = RenderRequest::fit(w, h);
            let full_lf = lightcraft_pipeline::render(&loaded.image, lf_info, lf_settings, &req).image;
            corner_cols.push(full_lf.crop(0, 0, 600.min(w), 600.min(h)));
        }

        if let Some((_, ref sy_settings, ref sy_info)) = render_sony {
            let (w, h) = export::full_size(&loaded.image, sy_settings);
            let req = RenderRequest::fit(w, h);
            let full_sy = lightcraft_pipeline::render(&loaded.image, sy_info, sy_settings, &req).image;
            corner_cols.push(full_sy.crop(0, 0, 600.min(w), 600.min(h)));
        }

        if let Some(ref lr) = lr_tiff {
            corner_cols.push(lr.crop(0, 0, 600.min(lr.width), 600.min(lr.height)));
        }

        corner_cols.push(camera_img.crop(0, 0, 600.min(camera_img.width), 600.min(camera_img.height)));

        let corner_sheet = side_by_side(&corner_cols, 8);
        png(&out_dir.join(format!("{stem}-100.png")), &corner_sheet)?;
    }

    Ok(())
}

fn main() {
    let mut args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let mut out_dir: Option<PathBuf> = None;

    if let Some(i) = args.iter().position(|a| a == Path::new("--out")) {
        args.remove(i);
        if i < args.len() {
            out_dir = Some(args.remove(i));
        } else {
            eprintln!("error: --out requires a directory argument");
            std::process::exit(2);
        }
    } else {
        let prefix = std::ffi::OsStr::new("--out=");
        if let Some(i) = args.iter().position(|a| a.as_os_str().as_encoded_bytes().starts_with(prefix.as_encoded_bytes())) {
            let arg = args.remove(i);
            let val = &arg.as_os_str().as_encoded_bytes()[prefix.len()..];
            out_dir = Some(PathBuf::from(std::str::from_utf8(val).unwrap_or_default()));
        }
    }

    if let Some(ref out) = out_dir
        && let Err(e) = std::fs::create_dir_all(out)
    {
        eprintln!("{}: {e}", out.display());
        std::process::exit(1);
    }

    let files = gather_raws(&args);
    if files.is_empty() {
        eprintln!("usage: lens <shoot folder or raws…> [--out DIR]");
        std::process::exit(2);
    }

    let mut summary = SummaryTracker::default();
    for file in &files {
        if let Err(e) = process_raw(file, out_dir.as_deref(), &mut summary) {
            println!("{}: failed: {e}", file.display());
        }
    }

    println!();
    summary.print();
}
