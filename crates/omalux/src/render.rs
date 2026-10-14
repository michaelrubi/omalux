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

use lightcraft_codecs::display::DisplayProfile;
use lightcraft_develop::DevelopSettings;
use lightcraft_pipeline::{DisplaySpace, PixelWindow, Quality, RenderRequest, SourceInfo, StageCache};
use lightcraft_raster::{Histogram, Rgb32f, Rgba8};
use omalux_engine::look::{self, Camera, Profile};
use omalux_engine::raw::Loaded;

/// The long edge the loupe's source is decoded to: enough for the loupe on
/// a 4K screen, and what M0's times were measured at. 100% zoom has its
/// own, full-size.
pub const SOURCE_EDGE: usize = 2560;
/// The box a draft is rendered into while a slider is dragged.
const DRAFT: (usize, usize) = (1152, 768);

/// A decoded raw: the pipeline's source, what settings are read against,
/// and what the camera did to its own JPEG.
type Decoded = Result<Loaded, String>;

/// How a raw's bytes become pictures: the app's is [`Decoder::raw`], a
/// test's whatever it needs.
#[derive(Clone, Copy)]
pub struct Decoder {
    /// The camera's embedded JPEG, for display.
    pub preview: fn(&[u8]) -> Option<Rgba8>,
    /// The raw itself, as the pipeline's source, with the look its
    /// profile starts from.
    pub source: fn(&[u8], Profile) -> Decoded,
    /// The raw at full size, for 100% zoom.
    pub full: fn(&[u8], Profile) -> Decoded,
    /// The long edge of the raw at full size, in pixels, without decoding
    /// it: how large 100% will be, before the full size is there.
    pub long_edge: fn(&[u8]) -> Option<usize>,
}

impl Decoder {
    pub fn raw() -> Self {
        Self {
            preview: |bytes| omalux_engine::raw::embedded_preview_srgb(bytes, SOURCE_EDGE),
            source: |bytes, profile| omalux_engine::raw::load(bytes, SOURCE_EDGE, profile),
            full: |bytes, profile| omalux_engine::raw::load(bytes, usize::MAX, profile),
            long_edge: |bytes| {
                let (w, h) = lightcraft_raw::probe_info(bytes).ok()?.developed_size();
                Some(w.max(h))
            },
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

impl Wanted {
    /// The profile its raw is developed for: its edit's, or a fresh
    /// raw's.
    fn profile(&self) -> Profile {
        self.settings.as_deref().map_or(Profile::Camera, Profile::of)
    }
}

/// What the loupe has room for, in pixels, and whether a slider is being
/// dragged (drafts, of the current frame only).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct View {
    pub size: (usize, usize),
    pub dragging: bool,
    /// The clipping warnings are on: clipped pixels are coloured.
    pub clipping: bool,
    /// The loupe is at 100%, with this point of the current frame in its
    /// middle (a share of the frame's width and height): the window of the
    /// frame that's on screen is rendered from the raw at full size.
    pub zoom: Option<[f32; 2]>,
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

/// Which window of a frame at 100% a render is: where in the frame (x, y,
/// width, height, in the full-size frame's pixels), and of what settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZoomKey {
    pub settings: u64,
    pub window: [usize; 4],
    /// The whole frame's size at 100%.
    pub full: (usize, usize),
    pub draft: bool,
    pub clipping: bool,
}

pub enum Done {
    /// The camera's JPEG of a frame: what the loupe shows until its render
    /// is in.
    Preview { path: PathBuf, image: egui::ColorImage },
    /// A frame's raw is decoded: what its settings are read against, and
    /// how large the frame is at 100% (near enough, until it's rendered
    /// there).
    Decoded { path: PathBuf, info: Arc<SourceInfo>, camera: Arc<Camera>, full: (usize, usize) },
    /// The window of the current frame that's on screen at 100%.
    Zoomed { path: PathBuf, key: ZoomKey, image: egui::ColorImage },
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

/// The monitor's colours: its profile, and the same as the pipeline's
/// target, so frames are rendered into its primaries.
#[derive(Clone)]
struct Display {
    profile: Arc<DisplayProfile>,
    space: DisplaySpace,
}

impl Display {
    /// A camera's JPEG, in sRGB, as the monitor's device values.
    fn preview(&self, image: &mut Rgba8) {
        if let Err(e) = self.profile.from_srgb(image) {
            log::warn!("display profile: {e}");
        }
    }

    /// A render into the monitor's primaries as its device values, in
    /// place; and its histogram, which is of the frame in sRGB whatever
    /// it's shown on. As Lightcraft's `finish_render` (`engine/src/display.rs`
    /// at `701f78e`): only the pixels the histogram reads are converted.
    fn finish(&self, image: &mut Rgba8) -> Option<Histogram> {
        let step = ((image.len() as f64 / 1_000_000.0).sqrt().ceil() as usize).max(1);
        let rows = (0..image.height).step_by(step);
        let sample: Vec<[u8; 4]> = rows.flat_map(|y| (0..image.width).step_by(step).map(move |x| (x, y))).map(|(x, y)| image.get(x, y)).collect();
        let sample = Rgba8 { width: image.width.div_ceil(step), height: image.height.div_ceil(step), data: sample };
        let histogram = self.profile.to_srgb(&sample).map(|srgb| Histogram::of_srgb8(&srgb));
        if let Err(e) = self.profile.correct(image) {
            log::warn!("display profile: {e}");
        }
        histogram.map_err(|e| log::warn!("display profile: {e}")).ok()
    }
}

/// A window to render at 100%: whose, from what, with which stages and
/// settings, and which window.
type ZoomJob = (PathBuf, Arc<Source>, Arc<StageCache>, Arc<DevelopSettings>, ZoomKey);

struct Source {
    image: Arc<Rgb32f>,
    info: Arc<SourceInfo>,
    camera: Arc<Camera>,
    /// Developed with the look fitted to its own JPEG, as an edit from
    /// before M4 wants it, and not with its camera's
    /// ([`Profile::fitted`]).
    fitted: bool,
}

impl Source {
    fn new(loaded: Loaded, profile: Profile) -> Self {
        Self { image: Arc::new(loaded.image), info: Arc::new(loaded.info), camera: Arc::new(loaded.camera), fitted: profile.fitted() }
    }

    /// Whether this is the development `frame`'s profile wants.
    fn suits(&self, frame: &Wanted) -> bool {
        self.fitted == frame.profile().fitted()
    }

    fn fresh(&self) -> Arc<DevelopSettings> {
        Arc::new(look::fresh(self.info.as_shot_temp, self.info.as_shot_tint))
    }
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
    /// The current frame at full size, while the loupe is at 100%, with
    /// its own stages.
    full: Option<(PathBuf, Arc<Source>, Arc<StageCache>)>,
    /// The frame being decoded at full size, and one that couldn't be.
    full_decoding: Option<PathBuf>,
    full_failed: Option<PathBuf>,
    /// The last window rendered at 100%.
    zoomed: Option<ZoomKey>,
    /// The monitor's colours, where they aren't sRGB's.
    display: Option<Display>,
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
        let current = frames.first().map(|f| &f.path);
        if state.stages.as_ref().is_some_and(|(path, _)| Some(path) != current) {
            state.stages = None;
        }
        // The full size is the current frame's, and only while it's zoomed.
        if view.zoom.is_none() || state.full.as_ref().is_some_and(|(path, ..)| Some(path) != current) {
            (state.full, state.zoomed) = (None, None);
        }
        if state.full_failed.as_ref() != current {
            state.full_failed = None;
        }
        state.wanted = frames;
        state.view = view;
        drop(state);
        self.shared.changed.notify_all();
    }

    /// Show everything in this monitor's colours from now on (`None`:
    /// sRGB). What was decoded and rendered for another is done again.
    pub fn set_display(&self, profile: Option<Arc<DisplayProfile>>) {
        let display = profile.and_then(|profile| {
            let space = DisplaySpace::new(profile.to_rec2020, profile.id);
            if space.is_none() {
                log::warn!("display profile {}: its primaries can't be used", profile.description);
            }
            Some(Display { space: space?, profile })
        });
        let mut state = self.shared.lock();
        if state.display.as_ref().map(|d| d.profile.id) == display.as_ref().map(|d| d.profile.id) {
            return;
        }
        state.display = display;
        state.sources.clear();
        state.rendered.clear();
        (state.stages, state.full, state.zoomed) = (None, None, None);
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
    pub fn busy(&self) -> bool {
        let state = self.shared.lock();
        let waiting = state.next_decode().is_some() || state.next_render().is_some() || state.next_full().is_some() || state.next_zoom().is_some();
        !state.decoding.is_empty() || state.full_decoding.is_some() || state.rendering || !state.done.is_empty() || waiting
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
    /// The most wanted frame that's yet to be decoded, or was for
    /// another profile's look than it wants now.
    fn next_decode(&self) -> Option<(PathBuf, Profile)> {
        let waiting = |frame: &&Wanted| {
            let decoded = self.sources.get(&frame.path).is_some_and(|source| source.suits(frame));
            !decoded && !self.decoding.contains(&frame.path) && !self.failed.contains(&frame.path)
        };
        self.wanted.iter().find(waiting).map(|frame| (frame.path.clone(), frame.profile()))
    }

    /// The current frame, if it's to be decoded at full size: the loupe is
    /// at 100% and its source is in, so its fit render comes first.
    fn next_full(&self) -> Option<(PathBuf, Profile)> {
        self.view.zoom?;
        let frame = self.wanted.first()?;
        let path = &frame.path;
        let have = self.full.as_ref().is_some_and(|(full, source, _)| full == path && source.suits(frame));
        let tried = self.full_decoding.as_ref() == Some(path) || self.full_failed.as_ref() == Some(path);
        let ready = self.sources.get(path).is_some_and(|source| source.suits(frame));
        (!have && !tried && ready).then(|| (path.clone(), frame.profile()))
    }

    /// The window of the current frame on screen at 100%, if it isn't
    /// what was last rendered there.
    fn next_zoom(&self) -> Option<ZoomJob> {
        let centre = self.view.zoom?;
        let frame = self.wanted.first()?;
        let (path, full, stages) = self.full.as_ref().filter(|(path, full, _)| *path == frame.path && full.suits(frame))?;
        let settings = frame.settings.clone().unwrap_or_else(|| full.fresh());
        let (frame_w, frame_h) = omalux_engine::export::full_size(&full.image, &settings);
        let (w, h) = (self.view.size.0.min(frame_w), self.view.size.1.min(frame_h));
        if w == 0 || h == 0 {
            return None;
        }
        // The window round the centre, moved as little as keeps it in the frame.
        let corner = |centre: f32, frame: usize, size: usize| ((centre * frame as f32) as usize).saturating_sub(size / 2).min(frame - size);
        let window = [corner(centre[0], frame_w, w), corner(centre[1], frame_h, h), w, h];
        let key = ZoomKey { settings: settings.hash64(), window, full: (frame_w, frame_h), draft: self.view.dragging, clipping: self.view.clipping };
        (self.zoomed != Some(key)).then(|| (path.clone(), full.clone(), stages.clone(), settings, key))
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
        // The current frame's render comes first: a neighbour that was
        // decoded sooner waits for it.
        let current = self.wanted.first()?;
        let decoded = self.sources.get(&current.path).is_some_and(|source| source.suits(current));
        if !decoded && !self.failed.contains(&current.path) {
            return None;
        }
        self.wanted.iter().enumerate().find_map(|(index, frame)| {
            let source = self.sources.get(&frame.path).filter(|source| source.suits(frame))?;
            let settings = frame.settings.clone().unwrap_or_else(|| source.fresh());
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
        /// A raw to decode: for the loupe, or at full size for 100%.
        enum Job {
            Source(PathBuf, Profile),
            Full(PathBuf, Profile),
        }
        let claim = |state: &mut State| {
            // The current frame's source, then its full size, then the neighbours.
            let current = state.wanted.first().map(|f| f.path.clone());
            let next = state.next_decode();
            if let Some((path, profile)) = next.clone().filter(|(path, _)| Some(path) == current.as_ref()) {
                state.decoding.insert(path.clone());
                return Some(Job::Source(path, profile));
            }
            if let Some((path, profile)) = state.next_full() {
                state.full_decoding = Some(path.clone());
                return Some(Job::Full(path, profile));
            }
            let (path, profile) = next?;
            state.decoding.insert(path.clone());
            Some(Job::Source(path, profile))
        };
        while let Some(job) = self.next(claim) {
            let (path, profile) = match job {
                Job::Source(path, profile) => (path, profile),
                Job::Full(path, profile) => {
                    let full = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|bytes| (self.decoder.full)(&bytes, profile));
                    self.finish(&path, |state, _| {
                        state.full_decoding = None;
                        // Still the current frame, and still zoomed?
                        let wanted = state.view.zoom.is_some() && state.wanted.first().is_some_and(|f| f.path == path);
                        match full {
                            Ok(loaded) if wanted => {
                                let source = Arc::new(Source::new(loaded, profile));
                                state.full = Some((path.clone(), source, Arc::new(StageCache::default())));
                            }
                            Ok(_) => {}
                            Err(error) if wanted => {
                                state.full_failed = Some(path.clone());
                                state.done.push(Done::Failed { path: path.clone(), error });
                            }
                            Err(_) => {}
                        }
                    });
                    continue;
                }
            };
            let bytes = std::fs::read(&path).map_err(|e| e.to_string());
            if let Ok(bytes) = &bytes
                && let Some(mut preview) = (self.decoder.preview)(bytes)
            {
                if let Some(display) = self.lock().display.clone() {
                    display.preview(&mut preview);
                }
                let image = color_image(&preview);
                self.finish(&path, |state, wanted| {
                    if wanted {
                        state.done.push(Done::Preview { path: path.clone(), image });
                    }
                });
            }
            let long_edge = bytes.as_ref().ok().and_then(|bytes| (self.decoder.long_edge)(bytes));
            let source = bytes.and_then(|bytes| (self.decoder.source)(&bytes, profile));
            self.finish(&path, |state, wanted| {
                state.decoding.remove(&path);
                match source {
                    Ok(loaded) if wanted => {
                        // The frame at 100%: the source's shape, as long as the raw's long edge.
                        let (w, h) = (loaded.image.width, loaded.image.height);
                        let scale = long_edge.map_or(1.0, |edge| edge as f64 / w.max(h).max(1) as f64);
                        let full = ((w as f64 * scale).round() as usize, (h as f64 * scale).round() as usize);
                        let source = Arc::new(Source::new(loaded, profile));
                        state.done.push(Done::Decoded { path: path.clone(), info: source.info.clone(), camera: source.camera.clone(), full });
                        state.sources.insert(path.clone(), source);
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
        /// A render to make: of a frame to fit the loupe, or of the window
        /// of the current frame that's on screen at 100%.
        enum Job {
            Fit(Key),
            Zoom(ZoomKey),
        }
        let claim = |state: &mut State| {
            let current = state.wanted.first().map(|f| f.path.clone());
            let fit = state.next_render();
            // The current frame to fit first (it's what's shown until the
            // window at 100% is in), then that window, then the neighbours.
            let zoom = if fit.as_ref().is_some_and(|(frame, ..)| Some(&frame.path) == current.as_ref()) { None } else { state.next_zoom() };
            state.rendering = true;
            let display = state.display.clone();
            if let Some((path, source, stages, settings, key)) = zoom {
                state.zoomed = Some(key);
                return Some((path, source, settings, Some(stages), display, Job::Zoom(key)));
            }
            let Some((frame, source, settings, key)) = fit else {
                state.rendering = false;
                return None;
            };
            // The current frame's stages are kept between renders; a
            // neighbour is rendered once, so it has none to keep.
            let stages = (Some(&frame.path) == current.as_ref()).then(|| match &state.stages {
                Some((path, stages)) if *path == frame.path => stages.clone(),
                _ => {
                    let stages = Arc::new(StageCache::default());
                    state.stages = Some((frame.path.clone(), stages.clone()));
                    stages
                }
            });
            // Claimed: not picked again while it's being rendered.
            state.rendered.insert(frame.path.clone(), key);
            Some((frame.path, source, settings, stages, display, Job::Fit(key)))
        };
        while let Some((path, source, settings, stages, display, job)) = self.next(claim) {
            let (draft, clipping, request) = match job {
                Job::Fit(key) => (key.draft, key.clipping, RenderRequest::fit(key.size.0, key.size.1)),
                Job::Zoom(key) => {
                    let [x, y, w, h] = key.window;
                    let request = RenderRequest { window: Some(PixelWindow { x, y, w, h }), ..RenderRequest::fit(key.full.0, key.full.1) };
                    (key.draft, key.clipping, request)
                }
            };
            let quality = if draft { Quality::Draft } else { Quality::Full };
            let request = RenderRequest { quality, display: display.as_ref().map(|d| d.space), ..request };
            let started = Instant::now();
            // What the profile and HDR put under the sliders is folded in here.
            let (folded, info) = look::effective(&settings, &source.info, &source.camera);
            let on_gpu = self.gpu.then(|| lightcraft_gpu::try_render(&source.image, &info, &folded, &request, stages.as_deref()).ok()).flatten();
            let mut rendered = on_gpu.unwrap_or_else(|| match &stages {
                Some(stages) => lightcraft_pipeline::render_cached(&source.image, &info, &folded, &request, stages),
                None => lightcraft_pipeline::render(&source.image, &info, &folded, &request),
            });
            drop((folded, info));
            let ms = started.elapsed().as_secs_f64() * 1e3;
            if clipping {
                crate::histogram::clipping_overlay(&mut rendered.image);
            }
            // On a monitor with a profile, the render is in its primaries:
            // take it to its device values, and the histogram back to sRGB.
            if let Some(histogram) = display.as_ref().and_then(|d| d.finish(&mut rendered.image)) {
                rendered.histogram = histogram;
            }
            let image = color_image(&rendered.image);
            self.finish(&path, |state, wanted| {
                state.rendering = false;
                match job {
                    Job::Fit(key) if wanted => state.done.push(Done::Rendered { path: path.clone(), key, image, histogram: rendered.histogram, settings, ms }),
                    // Only if it's still the current frame's, and still zoomed.
                    Job::Zoom(key) if state.zoomed == Some(key) && state.full.as_ref().is_some_and(|(full, ..)| *full == path) => {
                        log::debug!("{}: {}×{} at 100% in {ms:.0} ms", path.display(), key.window[2], key.window[3]);
                        state.done.push(Done::Zoomed { path: path.clone(), key, image });
                    }
                    _ => {}
                }
            });
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    use std::time::Duration;

    /// A raw as decoded, shot in Standard with DRO off.
    fn loaded(image: Rgb32f) -> Loaded {
        let info = SourceInfo { raw: true, as_shot_temp: 5200.0, as_shot_tint: 3.0, ..SourceInfo::default() };
        let camera = Camera { style: Some("Standard".into()), ..Camera::default() };
        Loaded { image, info, camera, timings: Default::default() }
    }

    /// A decoder that makes a small picture of any file, with its embedded
    /// JPEG a quarter the size.
    pub(crate) fn decoder() -> Decoder {
        Decoder {
            preview: |bytes| {
                let mut image = Rgba8::new(12, 8);
                image.data.fill([10, 20, 30, 255]);
                (bytes != b"no preview").then_some(image)
            },
            source: |bytes, _| match bytes {
                b"broken" => Err("not a raw".into()),
                _ => {
                    let mut image = Rgb32f::new(48, 32);
                    image.data.fill([0.18; 3]);
                    Ok(loaded(image))
                }
            },
            // Four times the source's size.
            full: |_, _| {
                let mut image = Rgb32f::new(192, 128);
                image.data.fill([0.18; 3]);
                Ok(loaded(image))
            },
            long_edge: |_| Some(192),
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
                Done::Preview { path, .. } | Done::Decoded { path, .. } | Done::Rendered { path, .. } | Done::Zoomed { path, .. } | Done::Failed { path, .. } => {
                    path.ends_with(name)
                }
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
            let mut settings = look::fresh(5200.0, 3.0);
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
        let source = Source::new(loaded(Rgb32f::new(2560, 1707)), Profile::Camera);
        let settings = DevelopSettings::default();
        let mut state = State { view: View { size: (3000, 1500), ..View::default() }, ..State::default() };
        assert_eq!(state.key(0, &source, &settings).size, (2560, 1500));
        state.view.dragging = true;
        assert_eq!((state.key(0, &source, &settings).size, state.key(0, &source, &settings).draft), (DRAFT, true));
        assert_eq!((state.key(1, &source, &settings).size, state.key(1, &source, &settings).draft), ((2560, 1500), false), "a neighbour");
    }

    #[test]
    fn at_100_percent_the_window_on_screen_is_rendered_from_the_full_size() {
        let folder = Folder::new("zoom", &[("a.ARW", ""), ("b.ARW", "")]);
        let renderer = Renderer::new(decoder(), false, || {});
        let frames = |names: &[&str]| names.iter().map(|name| wanted(&folder.0, name)).collect::<Vec<_>>();
        let zooms = |done: &[Done]| done.iter().filter_map(|d| if let Done::Zoomed { key, image, .. } = d { Some((key.window, key.full, image.size)) } else { None }).collect::<Vec<_>>();
        let view = View { size: (40, 30), ..View::default() };
        renderer.want(frames(&["a.ARW", "b.ARW"]), view);
        let done = settle(&renderer);
        assert!(zooms(&done).is_empty());
        assert!(done.iter().any(|d| matches!(d, Done::Decoded { full: (192, 128), .. })), "how large 100% will be is known from the start");

        // Zoomed at the middle: a loupe's worth of the full-size frame, round the middle.
        renderer.want(frames(&["a.ARW", "b.ARW"]), View { zoom: Some([0.5, 0.5]), ..view });
        assert_eq!(zooms(&settle(&renderer)), [([76, 49, 40, 30], (192, 128), [40, 30])]);
        // Panned to a corner: the window stops at the frame's edge.
        renderer.want(frames(&["a.ARW", "b.ARW"]), View { zoom: Some([1.0, 0.0]), ..view });
        assert_eq!(zooms(&settle(&renderer)), [([152, 0, 40, 30], (192, 128), [40, 30])]);
        assert!(renderer.shared.lock().full.is_some());

        // The next frame, still zoomed: its own full size. Zoomed out: none is kept.
        renderer.want(frames(&["b.ARW", "a.ARW"]), View { zoom: Some([1.0, 0.0]), ..view });
        let done = settle(&renderer);
        assert!(matches!(&done[..], [Done::Zoomed { path, .. }] if path.ends_with("b.ARW")), "{}", done.len());
        renderer.want(frames(&["b.ARW", "a.ARW"]), view);
        assert!(settle(&renderer).is_empty());
        assert!(renderer.shared.lock().full.is_none());
    }

    #[test]
    fn on_a_monitor_with_a_profile_frames_are_rendered_in_its_colours() {
        // A wide-gamut monitor: Display P3's primaries, as a profile.
        let p3 = lightcraft_codecs::icc::write_matrix_trc(&lightcraft_color::DISPLAY_P3, &lightcraft_codecs::Trc::Srgb);
        let profile = Arc::new(DisplayProfile::from_icc(&p3).unwrap());
        let folder = Folder::new("display", &[("a.ARW", "")]);
        let renderer = Renderer::new(decoder(), false, || {});
        let view = View { size: (24, 16), ..View::default() };
        let mut settings = look::fresh(5200.0, 3.0);
        settings.wb.temp = 9000.0;
        settings.wb.mode = lightcraft_develop::WbMode::Custom;
        let frames = || vec![Wanted { path: folder.0.join("a.ARW"), settings: Some(Arc::new(settings.clone())) }];
        let pixels = |done: &[Done]| {
            let preview = done.iter().find_map(|d| if let Done::Preview { image, .. } = d { Some(image.pixels[0]) } else { None });
            let render = done.iter().find_map(|d| if let Done::Rendered { image, histogram, .. } = d { Some((image.pixels[0], histogram.total)) } else { None });
            (preview, render)
        };
        renderer.want(frames(), view);
        let (srgb_preview, srgb_render) = pixels(&settle(&renderer));

        // The same frame again for the monitor: the JPEG's and the render's colours are
        // the monitor's (a warm grey has less red in P3's device values), the histogram still the frame's.
        renderer.set_display(Some(profile.clone()));
        let (preview, render) = pixels(&settle(&renderer));
        assert_ne!(preview, srgb_preview);
        let (srgb, p3) = (srgb_render.unwrap(), render.unwrap());
        assert!(p3.0.r() < srgb.0.r() && p3.1 == srgb.1, "{p3:?} {srgb:?}");
        // Set again, nothing is done again; back to sRGB, it is.
        renderer.set_display(Some(profile));
        assert!(settle(&renderer).is_empty());
        renderer.set_display(None);
        assert_eq!(pixels(&settle(&renderer)), (srgb_preview, srgb_render));
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
