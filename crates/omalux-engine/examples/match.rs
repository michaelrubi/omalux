//! M4's measure of the camera match: how close a fresh raw comes to the
//! camera's own JPEG, each way OmaLux can develop it (see "The look" and
//! "Quality bar" in docs/DESIGN.md).
//!
//!     OMALUX_CAMERA_PROFILES=/tmp/profiles cargo run --release -p omalux-engine --example match -- path/to/raws…
//!     OMALUX_CAMERA_PROFILES=/tmp/profiles cargo run --release -p omalux-engine --example match -- --list /tmp/profiles/held-out.txt
//!
//! For each raw, CIEDE2000 against its JPEG (mean, overall and on
//! saturated, skin and sky pixels, in the middle of the frame, away from
//! the JPEG's lens corrections) and how much darker or lighter it is in
//! L*, developed three ways:
//!
//! - **fitted**: the look fitted to the frame's own JPEG, as M1 to M3
//!   opened every frame;
//! - **profile**: the camera's profile for its Creative Style
//!   (`calibrate`'s), without DRO's lift;
//! - **camera**: the profile with DRO's lift, measured from the JPEG, under
//!   Shadows. This is Camera match, what a frame opens with.
//!
//! And at the end, their means over all the frames. `--list FILE` reads
//! the raws' paths from a file, one a line (`calibrate --every` writes
//! one). The raws are only read.

use std::path::{Path, PathBuf};

use lightcraft_pipeline::RenderRequest;
use lightcraft_raster::Rgba8;
use lightcraft_raster::resample::{Filter, resize};
use omalux_engine::compare::{self, Region, Summary};
use omalux_engine::look::{self, Camera, Profile};
use omalux_engine::raw;
use rayon::prelude::*;

/// The long edge the frames are developed and compared at.
const EDGE: usize = 1024;
/// The ways a frame is developed, in the order they're printed.
const WAYS: [&str; 3] = ["fitted", "profile", "camera"];

/// One frame, one way: its difference from the JPEG over each region.
type Measured = Vec<(Region, Summary)>;

struct Frame {
    name: String,
    /// The Creative Style and DRO, and the lift measured.
    shot: String,
    ways: [Measured; 3],
}

fn measure(path: &Path) -> Result<Frame, String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let reference = raw::embedded_preview_srgb(&bytes, EDGE).ok_or("no embedded JPEG")?;
    // The middle half of each side: the JPEG has Sony's distortion and shading corrections in it.
    let (w, h) = (reference.width, reference.height);
    let middle = |image: &Rgba8| image.crop(w / 4, h / 4, w / 2, h / 2);
    let comp = omalux_engine::sony::settings(&bytes).lens_comp;
    let against = |loaded: &raw::Loaded, profile: Profile, camera: &Camera| -> Result<Measured, String> {
        let mut fresh = look::fresh(loaded.info.as_shot_temp, loaded.info.as_shot_tint);
        profile.set(&mut fresh);
        fresh.optics.profile_distortion = if comp.distortion { 100.0 } else { 0.0 };
        fresh.optics.profile_vignetting = if comp.shading { 100.0 } else { 0.0 };
        fresh.optics.lens_profile = comp.distortion || comp.shading || comp.ca;
        let mut info = loaded.info.clone();
        if let Some(ref mut lens) = info.lens {
            if !comp.shading {
                lens.vignette = None;
            }
            if !comp.distortion && !comp.ca {
                lens.warp = None;
            } else if !comp.ca && let Some(ref mut warp) = lens.warp {
                warp.planes[0] = warp.planes[1];
                warp.planes[2] = warp.planes[1];
            }
        }
        let (settings, info) = look::effective(&fresh, &info, camera);
        let mut rendered = lightcraft_pipeline::render(&loaded.image, &info, &settings, &RenderRequest::fit(w, h)).image;
        if (rendered.width, rendered.height) != (w, h) {
            rendered = resize(&rendered.to_linear(), w, h, Filter::Box).to_srgb8();
        }
        compare::compare(&middle(&rendered), &middle(&reference))
    };
    let fitted = raw::load(&bytes, EDGE, Profile::Fitted)?;
    let camera = raw::load(&bytes, EDGE, Profile::Camera)?;
    let unlifted = Camera { lift: 0.0, ..camera.camera.clone() };
    let dro = camera.camera.dro.map_or("?".to_owned(), |dro| dro.label());
    let shot = format!(
        "{}, DRO {dro}, lift {:+.0}{}, lens comp {}",
        camera.camera.style.as_deref().unwrap_or("?"),
        camera.camera.lift,
        if camera.camera.fixed { "" } else { ", no profile" },
        comp.label()
    );
    let ways = [
        against(&fitted, Profile::Fitted, &fitted.camera)?,
        against(&camera, Profile::Camera, &unlifted)?,
        against(&camera, Profile::Camera, &camera.camera)?,
    ];
    Ok(Frame { name, shot, ways })
}

fn region(measured: &Measured, region: Region) -> Option<Summary> {
    measured.iter().find(|(r, s)| *r == region && s.share > 0.0).map(|(_, s)| *s)
}

fn raws(args: &[String]) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--list" {
            let list = args.next().ok_or("--list needs a file")?;
            let text = std::fs::read_to_string(list).map_err(|e| format!("{list}: {e}"))?;
            out.extend(text.lines().map(str::trim).filter(|line| !line.is_empty()).map(PathBuf::from));
            continue;
        }
        let path = PathBuf::from(arg);
        if path.is_dir() {
            let mut found: Vec<PathBuf> =
                std::fs::read_dir(&path).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("arw"))).collect();
            found.sort();
            out.extend(found);
        } else {
            out.push(path);
        }
    }
    Ok(out)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let files = match raws(&args) {
        Ok(files) if !files.is_empty() => files,
        Ok(_) => {
            eprintln!("usage: match <folder or raws…> [--list FILE]");
            std::process::exit(2);
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let measured: Vec<Result<Frame, String>> = files.par_iter().map(|path| measure(path).map_err(|e| format!("{}: {e}", path.display()))).collect();
    let mut frames = Vec::new();
    for frame in measured {
        match frame {
            Ok(frame) => {
                let ways: Vec<String> = WAYS
                    .iter()
                    .zip(&frame.ways)
                    .map(|(way, measured)| region(measured, Region::All).map_or(format!("{way} ?"), |s| format!("{way} {:.2} (L* {:+.1})", s.mean, s.lightness)))
                    .collect();
                println!("{}: {}: {}", frame.name, frame.shot, ways.join(", "));
                frames.push(frame);
            }
            Err(e) => eprintln!("{e}"),
        }
    }
    if frames.is_empty() {
        std::process::exit(1);
    }
    println!("\nΔE2000 against the camera's JPEG, mean over {} frames (and how much darker or lighter, in L*):", frames.len());
    for (i, way) in WAYS.iter().enumerate() {
        let line: Vec<String> = Region::ALL
            .into_iter()
            .filter_map(|r| {
                let held: Vec<Summary> = frames.iter().filter_map(|frame| region(&frame.ways[i], r)).collect();
                let n = held.len() as f64;
                (!held.is_empty()).then(|| format!("{} {:.2} ({:+.1}, {} frames)", r.label(), held.iter().map(|s| s.mean).sum::<f64>() / n, held.iter().map(|s| s.lightness).sum::<f64>() / n, held.len()))
            })
            .collect();
        println!("  {way:<8} {}", line.join(", "));
    }
}
