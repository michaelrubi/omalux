//! M0's last question: why the GPU loupe's times jump between frames.
//!
//!     cargo run --release -p omalux-engine --example idle -- a.ARW
//!
//! The loupe rendered on the GPU back to back, then after the GPU has sat
//! idle for a while, as it does while the next frame is decoded on the CPU;
//! then a slider drag from cold, on the GPU and the CPU. `OMALUX_HEARTBEAT=100`
//! gives the GPU something small to do every 100 ms while it would sit idle.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use lightcraft_develop::DevelopSettings;
use lightcraft_pipeline::RenderRequest;
use omalux_engine::look::Profile;
use omalux_engine::raw;

fn main() {
    let Some(path) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: idle <raw>");
        std::process::exit(2);
    };
    let bytes = std::fs::read(&path).expect("read");
    let raw::Loaded { image, info, .. } = raw::load(&bytes, 2560, Profile::Fitted).expect("decode");
    let source = Arc::new(image);
    let settings = DevelopSettings::for_raw(info.as_shot_temp, info.as_shot_tint);
    let loupe = RenderRequest::fit(1920, 1280);
    if !lightcraft_gpu::available() {
        eprintln!("no GPU: {}", lightcraft_gpu::unavailable_reason().unwrap_or_default());
        std::process::exit(1);
    }
    let render = || {
        let t = Instant::now();
        let ok = lightcraft_gpu::try_render(&source, &info, &settings, &loupe, None).is_ok();
        (t.elapsed().as_secs_f64() * 1e3, ok)
    };
    let line = |label: &str, times: &[(f64, bool)]| {
        let shown: Vec<String> = times.iter().map(|(ms, ok)| format!("{ms:.0}{}", if *ok { "" } else { "!" })).collect();
        println!("{label:>22}: {}", shown.join(" "));
    };
    // Something small for the GPU to do every `beat` ms while it would sit idle.
    let beat: Option<u64> = std::env::var("OMALUX_HEARTBEAT").ok().and_then(|v| v.parse().ok());
    let tiny = RenderRequest { quality: lightcraft_pipeline::Quality::Draft, ..RenderRequest::fit(96, 64) };
    let idle = |ms: u64| match beat {
        None => std::thread::sleep(Duration::from_millis(ms)),
        Some(beat) => {
            let until = Instant::now() + Duration::from_millis(ms);
            while Instant::now() < until {
                std::thread::sleep(Duration::from_millis(beat.min(ms)));
                drop(lightcraft_gpu::try_render(&source, &info, &settings, &tiny, None));
            }
        }
    };
    let back_to_back: Vec<_> = (0..12).map(|_| render()).collect();
    line("back to back", &back_to_back);
    for gap in [50, 200, 500, 1000, 2000, 5000] {
        let times: Vec<_> = (0..4)
            .map(|_| {
                idle(gap);
                render()
            })
            .collect();
        line(&format!("after {gap} ms idle"), &times);
    }
    // The GPU idle while the CPU is busy, as during a decode.
    let times: Vec<_> = (0..4)
        .map(|_| {
            let until = Instant::now() + Duration::from_millis(1000);
            while Instant::now() < until {
                drop(raw::load(&bytes, 2560, Profile::Fitted));
            }
            render()
        })
        .collect();
    line("after 1 s of decoding", &times);
    // A run of three after an idle spell: how long the slowness lasts.
    idle(3000);
    let run: Vec<_> = (0..8).map(|_| render()).collect();
    line("3000 ms idle, then run", &run);

    // A drag from cold: drafts at 60 a second, as a slider gives them, on the
    // GPU and on the CPU.
    let draft = RenderRequest { quality: lightcraft_pipeline::Quality::Draft, ..RenderRequest::fit(1152, 768) };
    for gpu in [true, false] {
        let cache = lightcraft_pipeline::StageCache::default();
        let mut drag = |k: usize| {
            let mut s = settings.clone();
            s.light.exposure = 0.3 + k as f64 * 0.01;
            let t = Instant::now();
            let ok = if gpu {
                lightcraft_gpu::try_render(&source, &info, &s, &draft, Some(&cache)).is_ok()
            } else {
                drop(lightcraft_pipeline::render_cached(&source, &info, &s, &draft, &cache));
                true
            };
            let ms = t.elapsed().as_secs_f64() * 1e3;
            std::thread::sleep(Duration::from_secs_f64((16.0 - ms).max(0.0) / 1e3));
            (ms, ok)
        };
        drag(0);
        std::thread::sleep(Duration::from_millis(3000));
        let times: Vec<_> = (1..=90).map(&mut drag).collect();
        line(if gpu { "drag from cold, GPU" } else { "drag from cold, CPU" }, &times);
    }
}
