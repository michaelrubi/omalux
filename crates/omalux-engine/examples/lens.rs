//! M0's lens spike: what lensfun's database and Sony's own data say about
//! each frame's lens, and the frame developed with lensfun's correction
//! and without, to judge on straight lines and a plain sky against
//! Lightroom's profile correction.
//!
//!     cargo run --release -p omalux-engine --example lens -- path/to/shoot --out /tmp/lens
//!
//! lensfun's database is the system's (`pacman -S lensfun`), or the folder
//! in `OMALUX_LENSFUN_DIR`. Sony's distortion table (raw IFD tag `0x7037`)
//! is reported, not applied: Lightcraft applies it on the ILCE-7RM4A only,
//! and trying it on the ILCE-7M3 means opening that gate in the vendored
//! `raw` crate (`vendor/arw_lens.rs`) on a test branch.

use std::path::{Path, PathBuf};

use lightcraft_develop::DevelopSettings;
use lightcraft_pipeline::RenderRequest;
use lightcraft_tiff::{Tiff, Value};
use omalux_engine::{lens_profiles, raw};

const SONY_DISTORTION: u16 = 0x7037;
const EDGE: usize = 2560;

/// Sony's distortion table, wherever in the file it is.
fn sony_table(bytes: &[u8]) -> Option<Vec<i16>> {
    let tiff = Tiff::parse(bytes).ok()?;
    tiff.all_ifds().into_iter().find_map(|ifd| match &ifd.get(SONY_DISTORTION)?.value {
        Value::SShort(v) => Some(v.clone()),
        _ => None,
    })
}

fn png(path: &Path, image: &lightcraft_raster::Rgba8) -> Result<(), String> {
    let bytes = lightcraft_codecs::encode::encode_png(&lightcraft_codecs::EncodeImage::rgba8(image), &Default::default()).map_err(|e| e.to_string())?;
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn lens(path: &Path, out: Option<&Path>) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let m = lightcraft_meta::extract(&bytes);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    println!(
        "{name}: {} at {:.0} mm f/{:.1}",
        m.lens_model.as_deref().unwrap_or("no lens named"),
        m.focal_length.unwrap_or(0.0),
        m.f_number.unwrap_or(0.0)
    );
    let found = lens_profiles::database().map(|db| {
        let camera = db.find_camera(m.make.as_deref().unwrap_or_default(), m.model.as_deref().unwrap_or_default());
        m.lens_model.as_deref().and_then(|l| db.find_lens(m.lens_make.as_deref(), l, camera)).map(|p| p.models.first().cloned().unwrap_or_default())
    });
    match found {
        None => println!("  lensfun: no database (OMALUX_LENSFUN_DIR, or /usr/share/lensfun/version_1)"),
        Some(None) => println!("  lensfun: the lens isn't in the database"),
        Some(Some(model)) => println!("  lensfun: {model}"),
    }
    match sony_table(&bytes) {
        Some(table) => println!("  Sony's distortion table: {table:?}"),
        None => println!("  Sony's distortion table: none"),
    }
    let (src, info) = raw::load(&bytes, EDGE)?;
    match &info.lens {
        Some(l) => println!("  correction the pipeline gets: warp {}, vignetting {}", l.warp.is_some() || l.has_stages(), l.vignette.is_some()),
        None => println!("  correction the pipeline gets: none"),
    }
    if let Some(out) = out {
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let mut settings = DevelopSettings::for_raw(info.as_shot_temp, info.as_shot_tint);
        let request = RenderRequest::fit(EDGE, EDGE);
        png(&out.join(format!("{stem}-uncorrected.png")), &lightcraft_pipeline::render(&src, &info, &settings, &request).image)?;
        if info.lens.is_some() {
            settings.optics.lens_profile = true;
            png(&out.join(format!("{stem}-lensfun.png")), &lightcraft_pipeline::render(&src, &info, &settings, &request).image)?;
        }
    }
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
                let mut found: Vec<PathBuf> =
                    dir.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("arw"))).collect();
                found.sort();
                files.extend(found);
            }
            Err(_) => files.push(a.clone()),
        }
    }
    if files.is_empty() {
        eprintln!("usage: lens <shoot folder or raws…> [--out DIR]");
        std::process::exit(2);
    }
    for file in &files {
        if let Err(e) = lens(file, out.as_deref()) {
            println!("  failed: {e}");
        }
    }
}
