//! M4's camera profiles: one for each camera model and Creative Style,
//! fitted to the camera's own JPEGs over many raws (see "The look" in
//! docs/DESIGN.md).
//!
//!     cargo run --release -p omalux-engine --example calibrate -- --out /tmp/profiles path/to/shoots…
//!     cargo run --release -p omalux-engine --example calibrate -- --max 400 --every 5 --out /tmp/profiles path/to/shoots…
//!
//! Folders are searched for raws three levels down. Each raw gives its
//! colour pairs (its sensor's colours beside its JPEG's); pooled per model
//! and style, they give a colour matrix, a hue/saturation table, a tone
//! curve and a chroma curve, written to `--out` (else where OmaLux reads
//! them: `OMALUX_CAMERA_PROFILES`, or `~/.config/omalux/camera-profiles`)
//! as `<model>-<style>.json`. The raws are only read.
//!
//! `--max N` takes N raws spread over all that were found (0 for all of
//! them, the default 600). `--every K` leaves every K-th raw out and lists
//! those in `held-out.txt` beside the profiles, for the `match` example to
//! measure the profile on frames it wasn't fitted to.
//!
//! The pooling is Lightcraft's `lightcraft-cli calibrate` (MIT OR
//! Apache-2.0, `apps/lightcraft-cli/src/main.rs` at `701f78e`), lifted.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use omalux_engine::camera_profiles::{self, Pool};

/// At least this many photos for a profile, as Lightcraft asks.
const MIN_FILES: usize = 5;

fn raw_files(path: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if path.is_file() {
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("arw")) {
            out.push(path.to_path_buf());
        }
        return;
    }
    if depth > 3 {
        return;
    }
    for entry in std::fs::read_dir(path).into_iter().flatten().flatten() {
        raw_files(&entry.path(), out, depth + 1);
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let (mut max, mut every, mut out, mut inputs) = (600usize, 0usize, None::<PathBuf>, Vec::new());
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "--max" => max = value("--max")?.parse().map_err(|_| "--max needs a number")?,
            "--every" => every = value("--every")?.parse().map_err(|_| "--every needs a number")?,
            "--out" => out = Some(value("--out")?.into()),
            a if a.starts_with("--") => return Err(format!("unknown option `{a}`")),
            f => inputs.push(f.to_owned()),
        }
    }
    if inputs.is_empty() {
        return Err("usage: calibrate [--max N] [--every K] [--out DIR] <folders or raws…>".into());
    }
    let mut files = Vec::new();
    for input in &inputs {
        raw_files(Path::new(input), &mut files, 0);
    }
    files.sort();
    files.dedup();
    let found = files.len();
    // Held out before anything else, so the same raws are whatever `--max` is.
    let (mut held, mut fitted) = (Vec::new(), Vec::new());
    for (i, file) in files.into_iter().enumerate() {
        if every > 1 && i % every == every - 1 { held.push(file) } else { fitted.push(file) }
    }
    let kept = fitted.len();
    if max > 0 && kept > max {
        // spread over all folders (dates, scenes) rather than the first N
        fitted = (0..max).filter_map(|k| fitted.get(k * kept / max).cloned()).collect();
    }
    eprintln!("calibrate: {} of {found} raws, {} held out", fitted.len(), held.len());
    let dir = out.or_else(camera_profiles::dir).ok_or("no profiles folder: pass --out DIR")?;
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 6);
    let (next, done) = (AtomicUsize::new(0), AtomicUsize::new(0));
    let pools: Vec<Pool> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut pool = Pool::default();
                    while let Some(path) = fitted.get(next.fetch_add(1, Ordering::Relaxed)) {
                        let result = std::fs::read(path).map_err(|e| e.to_string()).and_then(|bytes| pool.add(&bytes));
                        let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                        match result {
                            Ok(Some(_)) if n % 50 != 0 => {}
                            Ok(Some((model, style))) => eprintln!("[{n}/{}] {} ({model} {style})", fitted.len(), path.display()),
                            Ok(None) => eprintln!("[{n}/{}] {}: skipped (no usable camera JPEG or colour)", fitted.len(), path.display()),
                            Err(e) => eprintln!("[{n}/{}] {}: {e}", fitted.len(), path.display()),
                        }
                    }
                    pool
                })
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    });
    let mut pool = Pool::default();
    for p in pools {
        pool.merge(p);
    }
    let mut written = 0;
    for result in pool.fit(MIN_FILES) {
        match result {
            Ok(profile) => {
                let path = camera_profiles::save(&profile, &dir)?;
                let tone = match (profile.tone.is_some(), profile.tone_files) {
                    (false, _) => "no tone curve".to_owned(),
                    (true, 0) => "Sony's tone curve without DRO".to_owned(),
                    (true, n) => format!("its own tone curve from {n} photos without DRO"),
                };
                println!(
                    "{} {}: {} photos, {} colour pairs, hue/saturation table {}, {tone} → {}",
                    profile.model,
                    profile.style,
                    profile.files,
                    profile.samples,
                    profile.hue_sat.is_some(),
                    path.display()
                );
                written += 1;
            }
            Err(e) => eprintln!("calibrate: {e}"),
        }
    }
    for (model, style, n, without_dro) in pool.files() {
        if n < MIN_FILES {
            eprintln!("calibrate: {model} {style}: only {n} usable photo(s), at least {MIN_FILES} needed");
        } else if without_dro < MIN_FILES {
            eprintln!("calibrate: {model} {style}: {without_dro} photo(s) shot with DRO off, at least {MIN_FILES} needed for a tone curve of its own");
        }
    }
    if !held.is_empty() {
        let list: String = held.iter().map(|path| format!("{}\n", path.display())).collect();
        let path = dir.join("held-out.txt");
        std::fs::write(&path, list).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("{} raws held out, listed in {}", held.len(), path.display());
    }
    if written == 0 {
        return Err("no profile written".into());
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(&args) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
