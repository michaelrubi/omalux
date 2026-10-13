//! The reference renders the process version guards (see "The edit and the
//! sidecar" in docs/DESIGN.md): raws developed with settings that touch
//! every control OmaLux shows, kept as small PNGs. The test renders them
//! again and compares. If one changed and was meant to (a re-sync of
//! Lightcraft, a changed stage), an edit made before would now look
//! different: bump `sidecar::PROCESS_VERSION` and write the references
//! again:
//!
//!     OMALUX_WRITE_REFERENCE=1 cargo test -p omalux-engine reference
//!
//! The CC0 ILCE-7M3 raws' references are in `testdata/reference/` (the test
//! skips them without the raws: `scripts/fetch-corpus.sh`). Michael's own
//! frames stay outside the repo: with `OMALUX_REFERENCE` naming a folder of
//! raws, theirs are kept in its `omalux-reference/` folder.

use std::path::{Path, PathBuf};

use lightcraft_develop::{DevelopSettings, WbMode};
use lightcraft_pipeline::{RenderRequest, SourceInfo};
use lightcraft_raster::Rgba8;

use crate::edit::set_control;

/// The long edge the references are rendered at: enough to show a change
/// in tone or colour, small enough to keep in the repo.
const EDGE: usize = 256;
/// How far a render may be from its reference and still be the same
/// render: the CPU pipeline's own run-to-run differences are below this,
/// and a changed stage well above it.
const MAX_DIFFERENCE: u8 = 2;
const MEAN_DIFFERENCE: f64 = 0.25;

/// The settings each raw is rendered with, by name: a fresh raw's, and
/// edits that between them move every control M1 shows.
fn looks(info: &SourceInfo) -> Vec<(&'static str, DevelopSettings)> {
    let fresh = DevelopSettings::for_raw(info.as_shot_temp, info.as_shot_tint);
    let edit = |controls: &[(&str, f64)]| {
        let mut settings = fresh.clone();
        for (id, value) in controls {
            assert!(set_control(&mut settings, info, id, *value), "{id}");
        }
        settings
    };
    let warm = (info.as_shot_temp + 1500.0, info.as_shot_tint + 20.0);
    let mut as_shot_again = edit(&[("wb.temp", warm.0)]);
    crate::edit::as_shot(&mut as_shot_again, info);
    assert_eq!(as_shot_again.wb.mode, WbMode::AsShot);
    vec![
        ("fresh", fresh.clone()),
        ("light", edit(&[("light.exposure", 0.7), ("light.contrast", 25.0), ("light.highlights", -60.0), ("light.shadows", 45.0), ("light.whites", 15.0), ("light.blacks", -20.0)])),
        ("dark", edit(&[("light.exposure", -1.2), ("light.contrast", -30.0), ("light.highlights", 40.0), ("light.shadows", -35.0), ("light.whites", -25.0), ("light.blacks", 30.0)])),
        ("colour", edit(&[("wb.temp", warm.0), ("wb.tint", warm.1), ("color.vibrance", 40.0), ("color.saturation", -15.0)])),
        ("muted", edit(&[("wb.temp", info.as_shot_temp - 1200.0), ("wb.tint", info.as_shot_tint - 15.0), ("color.vibrance", -30.0), ("color.saturation", 25.0)])),
    ]
}

fn png(image: &Rgba8) -> Vec<u8> {
    lightcraft_codecs::encode::encode_png(&lightcraft_codecs::EncodeImage::rgba8(image), &Default::default()).expect("encode a PNG")
}

/// Max and mean |Δ| (8-bit, RGB) between two renders of one size.
fn difference(a: &Rgba8, b: &Rgba8) -> (u8, f64) {
    let (mut max, mut sum) = (0u8, 0u64);
    for (p, q) in a.data.iter().zip(&b.data) {
        for c in 0..3 {
            let d = p[c].abs_diff(q[c]);
            max = max.max(d);
            sum += u64::from(d);
        }
    }
    (max, sum as f64 / (a.data.len().max(1) * 3) as f64)
}

/// Render `raw` each way and compare with what's in `references` (or
/// write it there). Returns what differs.
fn check(name: &str, bytes: &[u8], references: &Path, write: bool) -> Vec<String> {
    let (source, info) = crate::raw::load(bytes, EDGE * 4).expect("decode");
    let mut problems = Vec::new();
    for (look, settings) in looks(&info) {
        let render = lightcraft_pipeline::render(&source, &info, &settings, &RenderRequest::fit(EDGE, EDGE)).image;
        let path = references.join(format!("{name}-{look}.png"));
        if write {
            std::fs::create_dir_all(references).expect("make the folder");
            std::fs::write(&path, png(&render)).expect("write a reference");
            continue;
        }
        let Ok(reference) = std::fs::read(&path) else {
            problems.push(format!("{}: no reference (write them: see reference.rs)", path.display()));
            continue;
        };
        let reference = lightcraft_codecs::decode(&reference, Default::default()).expect("a PNG").to_srgb8();
        if (reference.width, reference.height) != (render.width, render.height) {
            problems.push(format!("{name} {look}: {}×{}, the reference {}×{}", render.width, render.height, reference.width, reference.height));
            continue;
        }
        let (max, mean) = difference(&render, &reference);
        if max > MAX_DIFFERENCE || mean > MEAN_DIFFERENCE {
            problems.push(format!("{name} {look}: differs from its reference by up to {max} levels, {mean:.3} on average"));
        }
    }
    problems
}

#[test]
fn reference_renders_are_unchanged() {
    let write = std::env::var_os("OMALUX_WRITE_REFERENCE").is_some();
    let mut problems = Vec::new();
    // The CC0 corpus, with its references in the repo.
    let committed = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/reference");
    for (name, bytes) in crate::raw::tests::a7m3_corpus() {
        let name = name.trim_end_matches(".arw").to_owned();
        problems.extend(check(&name, &bytes, &committed, write));
    }
    // Michael's own frames, with their references beside them.
    if let Some(dir) = std::env::var_os("OMALUX_REFERENCE").map(PathBuf::from) {
        let mut raws: Vec<PathBuf> = std::fs::read_dir(&dir).expect("OMALUX_REFERENCE").flatten().map(|e| e.path()).collect();
        raws.retain(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("arw")));
        raws.sort();
        for raw in raws {
            let name = raw.file_stem().unwrap_or_default().to_string_lossy().into_owned();
            problems.extend(check(&name, &std::fs::read(&raw).expect("read"), &dir.join("omalux-reference"), write));
        }
    }
    assert!(
        problems.is_empty(),
        "these renders changed:\n  {}\nIf that's meant, edits made before now look different: bump sidecar::PROCESS_VERSION and write the references again (see reference.rs).",
        problems.join("\n  ")
    );
}

#[test]
fn a_changed_render_is_told_from_the_same_one() {
    let mut a = Rgba8::new(8, 8);
    a.data.fill([100, 120, 140, 255]);
    let mut b = a.clone();
    b.data[3] = [101, 120, 139, 255];
    assert_eq!(difference(&a, &b).0, 1);
    b.data.fill([104, 120, 140, 255]);
    let (max, mean) = difference(&a, &b);
    assert!(max > MAX_DIFFERENCE && mean > MEAN_DIFFERENCE, "a shift in one channel shows: {max}, {mean}");
}
