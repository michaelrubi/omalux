//! M0's render spike: real ARWs through Lightcraft's pipeline, on the CPU
//! and the GPU, measured.
//!
//!     cargo run --release -p omalux-engine --example spike -- path/to/shoot
//!     cargo run --release -p omalux-engine --example spike -- a.ARW b.ARW --out /tmp/spike
//!
//! For each raw: what the camera was set to (Creative Style, DRO), how
//! long decoding and developing take, how close the camera match comes to
//! the camera's own JPEG (CIEDE2000, mean and 95th percentile, overall and
//! on saturated, skin and sky pixels; the edges count too, where a
//! lens-corrected JPEG sits a pixel or two off the raw), the loupe and a
//! slider drag on the CPU and the GPU against DESIGN.md's 16 ms and 60 ms,
//! how far the two disagree, and the full-size export. With `--out`, the
//! export is written there as `<raw>.tif` (16-bit Linear ProPhoto), with
//! the render and the camera's JPEG beside it as PNGs to compare at 100%.
//!
//! `OMALUX_GPU=0` skips the GPU. Settings are a fresh raw's (Lightroom's
//! capture sharpening and colour noise reduction), white balance as shot.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use lightcraft_develop::DevelopSettings;
use lightcraft_pipeline::{PixelWindow, Quality, RenderRequest, SourceInfo, StageCache};
use lightcraft_raster::resample::{Filter, resize};
use lightcraft_raster::{Rgb32f, Rgba8};
use omalux_engine::compare::{self, Region};
use omalux_engine::look::{self, Camera, Profile};
use omalux_engine::{export, raw, sony};

/// The loupe: about 2.5 MP, as DESIGN.md's budget has it.
const LOUPE: (usize, usize) = (1920, 1280);
/// A draft while a slider is dragged.
const DRAFT: (usize, usize) = (1152, 768);
/// The source the loupe and drafts render from.
const PREVIEW_EDGE: usize = 2560;

/// Best wall-clock time of `n` runs, in milliseconds.
fn best(n: usize, mut f: impl FnMut()) -> f64 {
    (0..n)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed().as_secs_f64() * 1e3
        })
        .fold(f64::MAX, f64::min)
}

fn timed<T>(f: impl FnOnce() -> T) -> (T, f64) {
    let t = Instant::now();
    let out = f();
    (out, t.elapsed().as_secs_f64() * 1e3)
}

/// Max and mean |Δ| (8-bit, RGB) between two renders.
fn diff(a: &Rgba8, b: &Rgba8) -> (u8, f64) {
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

/// Why GPU renders fell back to the CPU, and how often: a fallback's CPU render
/// counts in the GPU's time.
static FALLBACKS: std::sync::Mutex<std::collections::BTreeMap<String, usize>> = std::sync::Mutex::new(std::collections::BTreeMap::new());

fn render(src: &Arc<Rgb32f>, info: &SourceInfo, s: &DevelopSettings, req: &RenderRequest, cache: Option<&StageCache>, gpu: bool) -> Rgba8 {
    if gpu {
        match lightcraft_gpu::try_render(src, info, s, req, cache) {
            Ok(r) => return r.image,
            Err(f) => {
                let reason = f.reason().unwrap_or("not tried").to_owned();
                *FALLBACKS.lock().unwrap_or_else(|e| e.into_inner()).entry(reason).or_default() += 1;
            }
        }
    }
    match cache {
        Some(cache) => lightcraft_pipeline::render_cached(src, info, s, req, cache).image,
        None => lightcraft_pipeline::render(src, info, s, req).image,
    }
}

fn png(path: &Path, image: &Rgba8) -> Result<(), String> {
    let bytes = lightcraft_codecs::encode::encode_png(&lightcraft_codecs::EncodeImage::rgba8(image), &Default::default()).map_err(|e| e.to_string())?;
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

#[derive(Default)]
struct Totals {
    frames: usize,
    styles: std::collections::BTreeMap<String, usize>,
    /// Mean ΔE2000 per region, per frame.
    de: Vec<[f64; 4]>,
    /// Mean ΔE2000 in the centre, per frame.
    centre: Vec<f64>,
    loupe: Vec<(f64, Option<f64>)>,
    drag: Vec<(f64, Option<f64>)>,
    export: Vec<f64>,
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v.get(v.len() / 2).copied().unwrap_or(f64::NAN)
}

fn spike(path: &Path, out: Option<&Path>, gpu: bool, totals: &mut Totals) -> Result<(), String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let meta = lightcraft_meta::extract(&bytes);
    let camera = sony::settings(&bytes);
    let style = camera.creative_style.clone().unwrap_or_else(|| "?".into());
    let dro = match (camera.dro, camera.dro_raw) {
        (Some(d), _) => d.label(),
        (None, Some(v)) => format!("tag {v}"),
        (None, None) => "?".into(),
    };
    println!("{name}: {} ISO {}, {}, {style}, DRO {dro}", meta.model.as_deref().unwrap_or("?"), meta.iso.unwrap_or(0), meta.lens_model.as_deref().unwrap_or("?"));
    *totals.styles.entry(format!("{style}, DRO {dro}")).or_default() += 1;

    // Decoding: the loupe's source, binned, and the full-size one for export and 100%.
    let (preview, t_preview) = timed(|| raw::load(&bytes, PREVIEW_EDGE, Profile::Camera, omalux_engine::sidecar::PROCESS_VERSION));
    let raw::Loaded { image: preview_img, info, camera: shot, timings: steps } = preview?;
    let (full, t_full) = timed(|| raw::load(&bytes, usize::MAX, Profile::Camera, omalux_engine::sidecar::PROCESS_VERSION));
    let full = full?.image;
    println!(
        "  decode: {t_preview:.0} ms for the {PREVIEW_EDGE} px source (raw {:.0}, camera match {:.0}, bin {:.0}, finish {:.0}), {t_full:.0} ms full size ({}×{})",
        steps.decode, steps.look, steps.demosaic, steps.finish, full.width, full.height
    );
    let look = match (shot.fixed, info.camera_tone.is_some()) {
        (true, _) => "its camera's profile",
        (false, true) => "fitted to the camera's JPEG",
        (false, false) => "none fitted (neutral fallback)",
    };
    let wb = if info.relative_wb { "relative".to_owned() } else { format!("{:.0} K / {:+.0}", info.as_shot_temp, info.as_shot_tint) };
    println!("  camera match: {look}; white balance as shot {wb}; lens data {}", if info.lens.is_some() { "found" } else { "none" });

    // What a frame opens with: Camera match, with what it puts under the sliders folded in.
    let fresh = look::fresh(info.as_shot_temp, info.as_shot_tint);
    let (settings, info) = look::effective(&fresh, &info, &shot);
    let (settings, info) = (settings.into_owned(), info.into_owned());
    println!("  profile: {}, DRO's lift {:+.0} under Shadows", Profile::of(&fresh).label(&shot), shot.lift);
    let preview = Arc::new(preview_img);

    // The camera match against the camera's JPEG, at the JPEG's size.
    let reference = raw::embedded_preview_srgb(&bytes, PREVIEW_EDGE).ok_or("no embedded JPEG")?;
    let mut matched = render(&preview, &info, &settings, &RenderRequest::fit(reference.width, reference.height), None, false);
    if (matched.width, matched.height) != (reference.width, reference.height) {
        matched = resize(&matched.to_linear(), reference.width, reference.height, Filter::Box).to_srgb8();
    }
    let summaries = compare::compare(&matched, &reference)?;
    let line: Vec<String> = summaries
        .iter()
        .filter(|(_, s)| s.share > 0.0)
        .map(|(r, s)| format!("{} {:.2} / {:.2} ({:.0} %, L* {:+.1})", r.label(), s.mean, s.p95, s.share * 100.0, s.lightness))
        .collect();
    println!("  ΔE2000 mean / p95 against the camera's JPEG: {}", line.join(", "));
    // The middle half of each side: the camera's JPEG has Sony's distortion and
    // shading corrections in it and this render has none, which costs most at the edges.
    let (w, h) = (reference.width, reference.height);
    let centre = |i: &Rgba8| i.crop(w / 4, h / 4, w / 2, h / 2);
    if let Some((_, s)) = compare::compare(&centre(&matched), &centre(&reference))?.into_iter().find(|(r, _)| *r == Region::All) {
        println!("  ΔE2000 mean / p95 in the centre (the middle half of each side): {:.2} / {:.2} (L* {:+.1})", s.mean, s.p95, s.lightness);
        totals.centre.push(s.mean);
    }
    totals.de.push(Region::ALL.map(|r| summaries.iter().find(|(s, _)| *s == r).map_or(f64::NAN, |(_, s)| if s.share > 0.0 { s.mean } else { f64::NAN })));

    // The loupe and a drag, CPU then GPU.
    let loupe = RenderRequest::fit(LOUPE.0, LOUPE.1);
    let draft = RenderRequest { quality: Quality::Draft, ..RenderRequest::fit(DRAFT.0, DRAFT.1) };
    let mut k = 0.0;
    let mut measure = |gpu: bool| {
        let cold = best(3, || drop(render(&preview, &info, &settings, &loupe, None, gpu)));
        let cache = StageCache::default();
        drop(render(&preview, &info, &settings, &draft, Some(&cache), gpu));
        let drag = best(9, || {
            let mut s = settings.clone();
            k += 1.0;
            s.light.exposure = 0.3 + k * 0.01;
            drop(render(&preview, &info, &s, &draft, Some(&cache), gpu));
        });
        (cold, drag)
    };
    let (cpu_loupe, cpu_drag) = measure(false);
    let gpu_times = gpu.then(|| measure(true));
    let gpu_line = match gpu_times {
        Some((l, d)) => {
            let (max, mean) = diff(&render(&preview, &info, &settings, &loupe, None, false), &render(&preview, &info, &settings, &loupe, None, true));
            format!("; GPU: loupe {l:.1} ms, drag {d:.1} ms; GPU against CPU: max {max}, mean {mean:.3} (8-bit)")
        }
        None => String::new(),
    };
    println!("  CPU: loupe {}×{} {cpu_loupe:.1} ms, drag (draft {}×{}) {cpu_drag:.1} ms{gpu_line}", LOUPE.0, LOUPE.1, DRAFT.0, DRAFT.1);
    totals.loupe.push((cpu_loupe, gpu_times.map(|t| t.0)));
    totals.drag.push((cpu_drag, gpu_times.map(|t| t.1)));

    // The export: full size, 16-bit Linear ProPhoto, on the CPU (Lightcraft's GPU renders 8-bit only).
    let (image, t_render) = timed(|| export::render(&full, &info, &Camera::default(), &settings));
    let image = image?;
    let (tiff, t_encode) = timed(|| export::tiff(&image));
    let tiff = tiff?;
    println!("  export: render {t_render:.0} ms, TIFF {t_encode:.0} ms, {:.0} MB", tiff.len() as f64 / 1e6);
    totals.export.push(t_render + t_encode);
    if let Some(out) = out {
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        std::fs::write(out.join(format!("{stem}.tif")), &tiff).map_err(|e| e.to_string())?;
        png(&out.join(format!("{stem}-omalux.png")), &matched)?;
        png(&out.join(format!("{stem}-camera.png")), &reference)?;
        // 100 %: a window at the centre of the full-size frame
        let (w, h) = export::full_size(&full, &settings);
        let (cw, ch) = (w.min(1600), h.min(1200));
        let window = PixelWindow { x: (w - cw) / 2, y: (h - ch) / 2, w: cw, h: ch };
        let request = RenderRequest { window: Some(window), ..RenderRequest::fit(w, h) };
        png(&out.join(format!("{stem}-100.png")), &render(&Arc::new(full), &info, &settings, &request, None, false))?;
    }
    totals.frames += 1;
    Ok(())
}

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
    let files = raws(&args);
    if files.is_empty() {
        eprintln!("usage: spike <shoot folder or raws…> [--out DIR]");
        std::process::exit(2);
    }
    let gpu = std::env::var("OMALUX_GPU").as_deref() != Ok("0") && lightcraft_gpu::available();
    match lightcraft_gpu::adapter_name().filter(|_| gpu) {
        Some(adapter) => println!("GPU: {adapter}"),
        None => println!("GPU: none ({})", lightcraft_gpu::unavailable_reason().unwrap_or_else(|| "OMALUX_GPU=0".into())),
    }
    let mut totals = Totals::default();
    for file in &files {
        if let Err(e) = spike(file, out.as_deref(), gpu, &mut totals) {
            println!("  failed: {e}");
        }
    }
    if totals.frames == 0 {
        return;
    }
    println!("\n{} frames", totals.frames);
    for (style, n) in &totals.styles {
        println!("  {n} × {style}");
    }
    let de: Vec<String> = Region::ALL
        .iter()
        .enumerate()
        .map(|(i, r)| format!("{} {:.2}", r.label(), median(totals.de.iter().map(|d| d[i]).filter(|v| v.is_finite()).collect())))
        .collect();
    println!("  median of the frames' mean ΔE2000: {}; centre {:.2}", de.join(", "), median(totals.centre.clone()));
    let col = |v: &[(f64, Option<f64>)], gpu: bool| median(v.iter().filter_map(|t| if gpu { t.1 } else { Some(t.0) }).collect());
    println!("  loupe: CPU {:.1} ms, GPU {:.1} ms (budget 60)", col(&totals.loupe, false), col(&totals.loupe, true));
    println!("  drag: CPU {:.1} ms, GPU {:.1} ms (budget 16)", col(&totals.drag, false), col(&totals.drag, true));
    println!("  export: {:.0} ms", median(totals.export.clone()));
    if gpu {
        let m = lightcraft_gpu::memory();
        println!("  GPU memory held: {:.0} MB allocated, {:.0} MB pooled", m.allocated as f64 / 1e6, m.pooled as f64 / 1e6);
        let fallbacks = FALLBACKS.lock().unwrap_or_else(|e| e.into_inner());
        if fallbacks.is_empty() {
            println!("  GPU renders that fell back to the CPU: none");
        }
        for (reason, n) in fallbacks.iter() {
            println!("  GPU renders that fell back to the CPU: {n} × {reason}");
        }
    }
}
