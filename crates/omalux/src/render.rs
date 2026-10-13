//! Rendering off the UI thread: the frames the loupe wants decoded and
//! developed in the order they're wanted, the current one first and its
//! neighbours prepared ahead.
//!
//! The app says what it wants every time that changes (the cursor moved, a
//! slider moved, the window was resized); that replaces whatever was still
//! waiting, so a frame the cursor has passed is never developed late, and
//! of a drag's drafts only the latest is rendered. The pattern is
//! Lightcraft's (`ui-egui/src/render.rs` at `701f78e`: a slot a view,
//! newer requests replacing queued ones, stand-ins until the render is
//! in), cut down to one view; the queue that's replaced whole is Omacull's
//! `loader.rs`.
//!
//! A frame's embedded JPEG comes back first, to show at once; its render
//! follows. Renders run on the GPU, and on the CPU where there's none or
//! it fails. The GPU clocks down after half a second idle and its first
//! renders after that take up to a quarter of a second (M0), which is why
//! the neighbours are rendered ahead and not when they're stepped to.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Instant;

use lightcraft_develop::DevelopSettings;
use lightcraft_pipeline::{Quality, RenderRequest, SourceInfo, StageCache};
use lightcraft_raster::{Histogram, Rgb32f, Rgba8};

/// The long edge the loupe's source is decoded to: enough for the loupe on
/// a 4K screen, and what M0's times were measured at. 100% zoom has its
/// own, full-size.
pub const SOURCE_EDGE: usize = 2560;
/// The box a draft is rendered into while a slider is dragged.
const DRAFT: (usize, usize) = (1152, 768);

/// A decoded raw: the pipeline's source, and what settings are read
/// against.
type Decoded = Result<(Rgb32f, SourceInfo), String>;

/// How a raw's bytes become pictures: the app's is [`Decoder::raw`], a
/// test's whatever it needs.
#[derive(Clone, Copy)]
pub struct Decoder {
    /// The camera's embedded JPEG, for display.
    pub preview: fn(&[u8]) -> Option<Rgba8>,
    /// The raw itself, as the pipeline's source.
    pub source: fn(&[u8]) -> Decoded,
}

impl Decoder {
    pub fn raw() -> Self {
        Self {
            preview: |bytes| omalux_engine::raw::embedded_preview_srgb(bytes, SOURCE_EDGE),
            source: |bytes| omalux_engine::raw::load(bytes, SOURCE_EDGE),
        }
    }
}

/// A frame to show or prepare.
#[derive(Clone, Debug, PartialEq)]
pub struct Wanted {
    pub path: PathBuf,
    /// Its edit, or `None` for a frame with none yet: a fresh raw's
    /// settings, white balance as shot.
    pub settings: Option<Arc<DevelopSettings>>,
}

/// What the loupe has room for, in pixels, and whether a slider is being
/// dragged (drafts, of the current frame only).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct View {
    pub size: (usize, usize),
    pub dragging: bool,
    /// The clipping warnings are on: clipped pixels are coloured.
    pub clipping: bool,
}

/// Which render of a frame this is: of what settings, how large, how good.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub settings: u64,
    pub size: (usize, usize),
    pub draft: bool,
    /// Its clipped pixels are coloured.
    pub clipping: bool,
}

pub enum Done {
    /// The camera's JPEG of a frame: what the loupe shows until its render
    /// is in.
    Preview { path: PathBuf, image: egui::ColorImage },
    /// A frame's raw is decoded: what its settings are read against.
    Decoded { path: PathBuf, info: Arc<SourceInfo> },
    Rendered {
        path: PathBuf,
        key: Key,
        image: egui::ColorImage,
        histogram: Histogram,
        /// The settings it was rendered with: the frame's own, or a fresh
        /// raw's where it had none.
        settings: Arc<DevelopSettings>,
        /// How long the render took, in milliseconds.
        ms: f64,
    },
    Failed { path: PathBuf, error: String },
}

struct Source {
    image: Arc<Rgb32f>,
    info: Arc<SourceInfo>,
}

#[derive(Default)]
struct State {
    /// The current frame, then the ones to prepare, most wanted first.
    wanted: Vec<Wanted>,
    view: View,
    sources: HashMap<PathBuf, Arc<Source>>,
    /// Being decoded now.
    decoding: HashSet<PathBuf>,
    /// Couldn't be decoded: not tried again while they're wanted.
    failed: HashSet<PathBuf>,
    /// The last render made of each frame.
    rendered: HashMap<PathBuf, Key>,
    /// A render is under way.
    rendering: bool,
    /// The current frame's stages, which a drag's renders reuse.
    stages: Option<(PathBuf, Arc<StageCache>)>,
    done: Vec<Done>,
    quit: bool,
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    decoder: Decoder,
    gpu: bool,
    wake: Box<dyn Fn() + Send + Sync>,
}

pub struct Renderer {
    shared: Arc<Shared>,
    threads: Vec<JoinHandle<()>>,
}

/// The texture of a render: its pixels as egui wants them. Renders are
/// opaque, so this is a plain copy. Lightcraft's (`ui-egui/src/render.rs`).
fn color_image(image: &Rgba8) -> egui::ColorImage {
    let pixels = image.data.iter().map(|p| egui::Color32::from_rgb(p[0], p[1], p[2])).collect();
    egui::ColorImage { size: [image.width, image.height], source_size: egui::vec2(image.width as f32, image.height as f32), pixels }
}

impl Renderer {
    /// `gpu`: render on the GPU where there is one. `wake` is called from
    /// the threads whenever something is done.
    pub fn new(decoder: Decoder, gpu: bool, wake: impl Fn() + Send + Sync + 'static) -> Self {
        let shared = Arc::new(Shared { state: Mutex::default(), changed: Condvar::new(), decoder, gpu, wake: Box::new(wake) });
        let spawn = |name: &str, run: fn(&Shared)| {
            let shared = shared.clone();
            std::thread::Builder::new().name(name.into()).spawn(move || run(&shared)).ok()
        };
        // Two to decode, so the frame just stepped to never waits for a
        // neighbour's decode to finish; one to render, as the GPU would
        // take them one at a time anyway.
        let threads = [spawn("decode", Shared::decode), spawn("decode", Shared::decode), spawn("render", Shared::render)];
        Self { shared, threads: threads.into_iter().flatten().collect() }
    }

    /// What to show and prepare from now on: `frames`' first is the current
    /// frame. Replaces what was wanted before; sources and renders of
    /// frames no longer wanted are dropped.
    pub fn want(&self, frames: Vec<Wanted>, view: View) {
        let mut state = self.shared.lock();
        if state.wanted == frames && state.view == view {
            return;
        }
        let keep = |path: &PathBuf| frames.iter().any(|f| f.path == *path);
        state.sources.retain(|path, _| keep(path));
        state.failed.retain(keep);
        state.rendered.retain(|path, _| keep(path));
        if state.stages.as_ref().is_some_and(|(path, _)| frames.first().is_none_or(|f| f.path != *path)) {
            state.stages = None;
        }
        state.wanted = frames;
        state.view = view;
        drop(state);
        self.shared.changed.notify_all();
    }

    /// A frame's decoded raw, if it's wanted and decoded: what the white
    /// balance selector samples.
    pub fn source(&self, path: &Path) -> Option<Arc<Rgb32f>> {
        self.shared.lock().sources.get(path).map(|source| source.image.clone())
    }

    /// What's been finished since this was last called.
    pub fn done(&self) -> Vec<Done> {
        std::mem::take(&mut self.shared.lock().done)
    }

    /// Whether there's anything still to decode or render, or anything
    /// finished that [`Self::done`] hasn't handed over yet.
    #[cfg(test)]
    pub fn busy(&self) -> bool {
        let state = self.shared.lock();
        !state.decoding.is_empty() || state.rendering || !state.done.is_empty() || state.next_decode().is_some() || state.next_render().is_some()
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        self.shared.lock().quit = true;
        self.shared.changed.notify_all();
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

impl State {
    /// The most wanted frame that's yet to be decoded.
    fn next_decode(&self) -> Option<PathBuf> {
        let waiting = |path: &PathBuf| !self.sources.contains_key(path) && !self.decoding.contains(path) && !self.failed.contains(path);
        self.wanted.iter().map(|f| &f.path).find(|path| waiting(path)).cloned()
    }

    /// The render a frame should have: a draft of the current one while a
    /// slider is dragged, otherwise as large as the loupe.
    fn key(&self, index: usize, source: &Source, settings: &DevelopSettings) -> Key {
        let draft = self.view.dragging && index == 0;
        // No larger than the source: there's nothing more in it to show.
        let (w, h) = (self.view.size.0.min(source.image.width), self.view.size.1.min(source.image.height));
        let size = if draft { (w.min(DRAFT.0), h.min(DRAFT.1)) } else { (w, h) };
        Key { settings: settings.hash64(), size, draft, clipping: self.view.clipping }
    }

    /// The most wanted frame that's decoded and whose render isn't the one
    /// it should have, with that render's key and settings.
    fn next_render(&self) -> Option<(Wanted, Arc<Source>, Arc<DevelopSettings>, Key)> {
        if self.view.size.0 == 0 || self.view.size.1 == 0 {
            return None;
        }
        self.wanted.iter().enumerate().find_map(|(index, frame)| {
            let source = self.sources.get(&frame.path)?;
            let fresh = || Arc::new(DevelopSettings::for_raw(source.info.as_shot_temp, source.info.as_shot_tint));
            let settings = frame.settings.clone().unwrap_or_else(fresh);
            let key = self.key(index, source, &settings);
            (self.rendered.get(&frame.path) != Some(&key)).then(|| (frame.clone(), source.clone(), settings, key))
        })
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Wait for the next job `find` sees, or `None` when it's time to stop.
    fn next<T>(&self, find: impl Fn(&mut State) -> Option<T>) -> Option<T> {
        let mut state = self.lock();
        loop {
            if state.quit {
                return None;
            }
            if let Some(job) = find(&mut state) {
                return Some(job);
            }
            state = self.changed.wait(state).unwrap_or_else(|e| e.into_inner());
        }
    }

    fn finish(&self, path: &Path, change: impl FnOnce(&mut State, bool)) {
        let mut state = self.lock();
        let wanted = state.wanted.iter().any(|f| f.path == path);
        change(&mut state, wanted);
        drop(state);
        self.changed.notify_all();
        (self.wake)();
    }

    fn decode(&self) {
        let claim = |state: &mut State| {
            let path = state.next_decode()?;
            state.decoding.insert(path.clone());
            Some(path)
        };
        while let Some(path) = self.next(claim) {
            let bytes = std::fs::read(&path).map_err(|e| e.to_string());
            if let Ok(bytes) = &bytes
                && let Some(preview) = (self.decoder.preview)(bytes)
            {
                let image = color_image(&preview);
                self.finish(&path, |state, wanted| {
                    if wanted {
                        state.done.push(Done::Preview { path: path.clone(), image });
                    }
                });
            }
            let source = bytes.and_then(|bytes| (self.decoder.source)(&bytes));
            self.finish(&path, |state, wanted| {
                state.decoding.remove(&path);
                match source {
                    Ok((image, info)) if wanted => {
                        let info = Arc::new(info);
                        state.sources.insert(path.clone(), Arc::new(Source { image: Arc::new(image), info: info.clone() }));
                        state.done.push(Done::Decoded { path: path.clone(), info });
                    }
                    Ok(_) => {}
                    Err(error) if wanted => {
                        state.failed.insert(path.clone());
                        state.done.push(Done::Failed { path: path.clone(), error });
                    }
                    Err(_) => {}
                }
            });
        }
    }

    fn render(&self) {
        let claim = |state: &mut State| {
            let (frame, source, settings, key) = state.next_render()?;
            // The current frame's stages are kept between renders; a
            // neighbour is rendered once, so it has none to keep.
            let current = state.wanted.first().is_some_and(|f| f.path == frame.path);
            let stages = current.then(|| match &state.stages {
                Some((path, stages)) if *path == frame.path => stages.clone(),
                _ => {
                    let stages = Arc::new(StageCache::default());
                    state.stages = Some((frame.path.clone(), stages.clone()));
                    stages
                }
            });
            // Claimed: not picked again while it's being rendered.
            state.rendered.insert(frame.path.clone(), key);
            state.rendering = true;
            Some((frame.path, source, settings, key, stages))
        };
        while let Some((path, source, settings, key, stages)) = self.next(claim) {
            let quality = if key.draft { Quality::Draft } else { Quality::Full };
            let request = RenderRequest { quality, ..RenderRequest::fit(key.size.0, key.size.1) };
            let started = Instant::now();
            let on_gpu = self.gpu.then(|| lightcraft_gpu::try_render(&source.image, &source.info, &settings, &request, stages.as_deref()).ok()).flatten();
            let rendered = on_gpu.unwrap_or_else(|| match &stages {
                Some(stages) => lightcraft_pipeline::render_cached(&source.image, &source.info, &settings, &request, stages),
                None => lightcraft_pipeline::render(&source.image, &source.info, &settings, &request),
            });
            let ms = started.elapsed().as_secs_f64() * 1e3;
            let mut rendered = rendered;
            if key.clipping {
                crate::histogram::clipping_overlay(&mut rendered.image);
            }
            let image = color_image(&rendered.image);
            self.finish(&path, |state, wanted| {
                state.rendering = false;
                if wanted {
                    state.done.push(Done::Rendered { path: path.clone(), key, image, histogram: rendered.histogram, settings, ms });
                }
            });
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    use std::time::Duration;

    /// A decoder that makes a small picture of any file, with its embedded
    /// JPEG a quarter the size.
    pub(crate) fn decoder() -> Decoder {
        Decoder {
            preview: |bytes| {
                let mut image = Rgba8::new(12, 8);
                image.data.fill([10, 20, 30, 255]);
                (bytes != b"no preview").then_some(image)
            },
            source: |bytes| match bytes {
                b"broken" => Err("not a raw".into()),
                _ => {
                    let mut image = Rgb32f::new(48, 32);
                    image.data.fill([0.18; 3]);
                    Ok((image, SourceInfo { raw: true, as_shot_temp: 5200.0, as_shot_tint: 3.0, ..SourceInfo::default() }))
                }
            },
        }
    }

    pub(crate) struct Folder(pub PathBuf);

    impl Folder {
        pub(crate) fn new(name: &str, files: &[(&str, &str)]) -> Self {
            let dir = std::env::temp_dir().join(format!("omalux-render-{name}-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            for (file, contents) in files {
                std::fs::write(dir.join(file), contents).unwrap();
            }
            Self(dir)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Everything done until the renderer has nothing left to do.
    pub(crate) fn settle(renderer: &Renderer) -> Vec<Done> {
        let mut done = Vec::new();
        for _ in 0..2000 {
            done.extend(renderer.done());
            if !renderer.busy() {
                return done;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("the renderer never finished");
    }

    fn wanted(dir: &Path, name: &str) -> Wanted {
        Wanted { path: dir.join(name), settings: None }
    }

    fn renders(done: &[Done]) -> Vec<(String, Key)> {
        let name = |path: &Path| path.file_name().unwrap().to_string_lossy().into_owned();
        done.iter().filter_map(|d| if let Done::Rendered { path, key, .. } = d { Some((name(path), *key)) } else { None }).collect()
    }

    #[test]
    fn a_frame_comes_back_as_its_preview_then_its_render_and_neighbours_follow() {
        let folder = Folder::new("order", &[("a.ARW", ""), ("b.ARW", ""), ("c.ARW", "broken")]);
        let renderer = Renderer::new(decoder(), false, || {});
        let view = View { size: (24, 16), ..View::default() };
        renderer.want(vec![wanted(&folder.0, "a.ARW"), wanted(&folder.0, "b.ARW"), wanted(&folder.0, "c.ARW")], view);
        let done = settle(&renderer);

        let of = |name: &str| -> Vec<&Done> {
            let path = |d: &&Done| match d {
                Done::Preview { path, .. } | Done::Decoded { path, .. } | Done::Rendered { path, .. } | Done::Failed { path, .. } => path.ends_with(name),
            };
            done.iter().filter(path).collect()
        };
        assert!(matches!(of("a.ARW")[..], [Done::Preview { .. }, Done::Decoded { .. }, Done::Rendered { .. }]));
        assert!(matches!(of("b.ARW")[..], [Done::Preview { .. }, Done::Decoded { .. }, Done::Rendered { .. }]));
        assert!(matches!(of("c.ARW")[..], [Done::Preview { .. }, Done::Failed { .. }]));
        // A fresh raw's settings, white balance as shot, at the loupe's size.
        let Done::Rendered { key, image, settings, .. } = of("a.ARW")[2] else { unreachable!() };
        assert_eq!((settings.wb.temp, settings.wb.tint, settings.detail.sharpen_amount), (5200.0, 3.0, 40.0));
        assert_eq!((image.size, key.size, key.draft), ([24, 16], (24, 16), false));

        // Asked again for the same, nothing is done again.
        renderer.want(vec![wanted(&folder.0, "a.ARW"), wanted(&folder.0, "b.ARW"), wanted(&folder.0, "c.ARW")], view);
        assert!(settle(&renderer).is_empty());
    }

    #[test]
    fn a_drag_gets_drafts_of_the_current_frame_and_a_full_render_when_it_ends() {
        let folder = Folder::new("drag", &[("a.ARW", ""), ("b.ARW", "")]);
        let renderer = Renderer::new(decoder(), false, || {});
        let frames = |exposure: f64| {
            let mut settings = DevelopSettings::for_raw(5200.0, 3.0);
            settings.light.exposure = exposure;
            vec![Wanted { path: folder.0.join("a.ARW"), settings: Some(Arc::new(settings)) }, wanted(&folder.0, "b.ARW")]
        };
        let size = (40, 30);
        renderer.want(frames(0.0), View { size, ..View::default() });
        assert_eq!(renders(&settle(&renderer)).iter().map(|(name, key)| (name.as_str(), key.draft)).collect::<Vec<_>>(), [("a.ARW", false), ("b.ARW", false)]);

        // Dragging: a draft of the current frame, small; the neighbour's render stands.
        renderer.want(frames(0.5), View { size, dragging: true, ..View::default() });
        let drafts = renders(&settle(&renderer));
        assert!(matches!(drafts[..], [(ref name, Key { draft: true, size: (40, 30), .. })] if name == "a.ARW"), "{drafts:?}");
        // Let go: the same settings at full quality.
        renderer.want(frames(0.5), View { size, ..View::default() });
        let full = renders(&settle(&renderer));
        assert!(matches!(full[..], [(ref name, Key { draft: false, size: (40, 30), .. })] if name == "a.ARW"), "{full:?}");
        assert_eq!(full[0].1.settings, drafts[0].1.settings);
    }

    #[test]
    fn drafts_are_small_and_nothing_is_larger_than_its_source() {
        let source = Source { image: Arc::new(Rgb32f::new(2560, 1707)), info: Arc::default() };
        let settings = DevelopSettings::default();
        let mut state = State { view: View { size: (3000, 1500), ..View::default() }, ..State::default() };
        assert_eq!(state.key(0, &source, &settings).size, (2560, 1500));
        state.view.dragging = true;
        assert_eq!((state.key(0, &source, &settings).size, state.key(0, &source, &settings).draft), (DRAFT, true));
        assert_eq!((state.key(1, &source, &settings).size, state.key(1, &source, &settings).draft), ((2560, 1500), false), "a neighbour");
    }

    #[test]
    fn what_is_no_longer_wanted_is_dropped() {
        let folder = Folder::new("dropped", &[("a.ARW", ""), ("b.ARW", "")]);
        let renderer = Renderer::new(decoder(), false, || {});
        let view = View { size: (24, 16), ..View::default() };
        renderer.want(vec![wanted(&folder.0, "a.ARW")], view);
        settle(&renderer);
        renderer.want(vec![wanted(&folder.0, "b.ARW")], view);
        settle(&renderer);
        let state = renderer.shared.lock();
        assert_eq!(state.sources.keys().collect::<Vec<_>>(), [&folder.0.join("b.ARW")]);
        assert_eq!(state.rendered.keys().collect::<Vec<_>>(), [&folder.0.join("b.ARW")]);
        drop(state);
        // Stepped back to: decoded and rendered again.
        renderer.want(vec![wanted(&folder.0, "a.ARW")], view);
        assert_eq!(renders(&settle(&renderer)).len(), 1);
    }
}
