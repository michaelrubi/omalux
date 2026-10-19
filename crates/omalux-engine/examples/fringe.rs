//! M3's check of Remove Chromatic Aberration and Defringe on real frames
//! (the f/1.8 ones, where fast primes fringe): for each raw, the lateral
//! CA Lightcraft estimates, and how much of the frame is fringed (purple
//! or green beside a blown highlight) as it opens, with Remove Chromatic
//! Aberration, and with Defringe too. `--out` writes the worst-fringed
//! part of each frame at 100%, the three side by side, to look at.
//!
//! ```bash
//! cargo run --release -p omalux-engine --example fringe -- path/to/raws --out /tmp/fringe
//! ```

use std::path::{Path, PathBuf};
use std::sync::Arc;

use lightcraft_develop::DevelopSettings;
use lightcraft_pipeline::{PixelWindow, RenderRequest};
use lightcraft_raster::Rgba8;
use omalux_engine::look::Profile;
use omalux_engine::raw;

/// The Defringe amounts tried: Lightroom's range is 0 to 20.
const DEFRINGE: f64 = 8.0;
/// The side of the part written at 100%.
const TILE: usize = 512;

/// Whether a pixel is fringe-coloured: saturated, and purple or green.
/// `0` no, `1` purple, `2` green.
fn fringe(p: [u8; 4]) -> u8 {
    let [r, g, b] = [f32::from(p[0]), f32::from(p[1]), f32::from(p[2])];
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    if max < 40.0 || (max - min) / max < 0.25 {
        return 0;
    }
    // Purple: green is the least, and blue isn't far under red. Green: it's the most.
    if g == min && b > 0.6 * r && r > 0.5 * b {
        1
    } else if g == max && g - r.max(b) > 0.12 * max {
        2
    } else {
        0
    }
}

/// For each tile of the frame, how many fringe-coloured pixels are within
/// three pixels of a blown one; and the totals, purple and green.
fn fringed(image: &Rgba8) -> (Vec<usize>, usize, [usize; 2]) {
    let (w, h) = (image.width, image.height);
    let blown: Vec<bool> = image.data.iter().map(|p| p[0].min(p[1]).min(p[2]) >= 250).collect();
    let across = w.div_ceil(TILE);
    let mut tiles = vec![0; across * h.div_ceil(TILE)];
    let mut totals = [0; 2];
    for y in 3..h.saturating_sub(3) {
        for x in 3..w.saturating_sub(3) {
            let kind = fringe(image.data[y * w + x]);
            if kind == 0 || blown[y * w + x] {
                continue;
            }
            let beside = (y - 3..=y + 3).any(|ny| (x - 3..=x + 3).any(|nx| blown[ny * w + nx]));
            if beside {
                tiles[y / TILE * across + x / TILE] += 1;
                totals[usize::from(kind - 1)] += 1;
            }
        }
    }
    (tiles, across, totals)
}

fn png(path: &Path, image: &Rgba8) -> Result<(), String> {
    let bytes = lightcraft_codecs::encode::encode_png(&lightcraft_codecs::EncodeImage::rgba8(image), &Default::default()).map_err(|e| e.to_string())?;
    std::fs::write(path, bytes).map_err(|e| e.to_string())
}

fn check(file: &Path, out: Option<&Path>) -> Result<(), String> {
    let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
    let raw::Loaded { image: full, info, .. } = raw::load(&bytes, usize::MAX, Profile::Fitted, omalux_engine::sidecar::PROCESS_VERSION)?;
    let full = Arc::new(full);
    let fresh = DevelopSettings::for_raw(info.as_shot_temp, info.as_shot_tint);
    let mut remove_ca = fresh.clone();
    remove_ca.optics.remove_ca = true;
    let mut defringed = remove_ca.clone();
    (defringed.optics.defringe_purple_amount, defringed.optics.defringe_green_amount) = (DEFRINGE, DEFRINGE);

    let (w, h) = omalux_engine::export::full_size(&full, &fresh);
    let whole = RenderRequest::fit(w, h);
    let render = |settings: &DevelopSettings, request: &RenderRequest| lightcraft_pipeline::render(&full, &info, settings, request).image;
    let [ca_red, ca_blue] = lightcraft_pipeline::optics::estimate_lateral_ca(&full);
    // The estimate is a magnification: this far out, at the frame's corner, in pixels.
    let corner = (w as f64).hypot(h as f64) / 2.0;
    println!("{}: lateral CA red {:+.2} px, blue {:+.2} px at the corners", file.display(), ca_red * corner, ca_blue * corner);

    let (tiles, across, as_opened) = fringed(&render(&fresh, &whole));
    let (_, _, with_ca) = fringed(&render(&remove_ca, &whole));
    let (_, _, with_defringe) = fringed(&render(&defringed, &whole));
    let per_million = |[purple, green]: [usize; 2]| format!("{:.0} purple, {:.0} green", purple as f64 * 1e6 / (w * h) as f64, green as f64 * 1e6 / (w * h) as f64);
    println!("  fringed pixels per million: as it opens {}; Remove CA {}; and Defringe {DEFRINGE} {}", per_million(as_opened), per_million(with_ca), per_million(with_defringe));

    let Some(out) = out else { return Ok(()) };
    let Some((worst, _)) = tiles.iter().enumerate().max_by_key(|(_, count)| **count).filter(|(_, count)| **count > 0) else {
        println!("  nothing fringed to write");
        return Ok(());
    };
    let window = PixelWindow { x: worst % across * TILE, y: worst / across * TILE, w: TILE, h: TILE }.clamped(w, h);
    let request = RenderRequest { window: Some(window), ..whole };
    let parts = [&fresh, &remove_ca, &defringed].map(|settings| render(settings, &request));
    let mut side_by_side = Rgba8::new(window.w * 3, window.h);
    for (i, part) in parts.iter().enumerate() {
        for y in 0..window.h.min(part.height) {
            for x in 0..window.w.min(part.width) {
                side_by_side.data[y * side_by_side.width + i * window.w + x] = part.data[y * part.width + x];
            }
        }
    }
    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("frame");
    let path = out.join(format!("{stem}-fringe.png"));
    png(&path, &side_by_side)?;
    println!("  {}: as it opens, Remove CA, and Defringe, at {},{}", path.display(), window.x, window.y);
    Ok(())
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
                let mut found: Vec<PathBuf> = dir.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("arw"))).collect();
                found.sort();
                files.extend(found);
            }
            Err(_) => files.push(a.clone()),
        }
    }
    if files.is_empty() {
        eprintln!("usage: fringe <folder or raws…> [--out DIR]");
        std::process::exit(2);
    }
    for file in &files {
        if let Err(e) = check(file, out.as_deref()) {
            println!("{}: failed: {e}", file.display());
        }
    }
}
