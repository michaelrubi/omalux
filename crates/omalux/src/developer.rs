//! Developed thumbnails, off the UI thread: the filmstrip's frames with an
//! edit, rendered small on the CPU or taken from the cache. One thread, so
//! they never take the decoding and rendering the loupe is waiting on; the
//! last asked for is made first, and what's asked of a frame again replaces
//! what was waiting.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use lightcraft_develop::DevelopSettings;
use lightcraft_pipeline::SourceInfo;
use lightcraft_raster::{Rgb32f, Rgba8};
use omalux_engine::look::{Camera, Profile};
use omalux_engine::thumbs;

use crate::render::Decoder;

/// A raw decoded already: its picture, and what its settings are read against.
pub type Source = (Arc<Rgb32f>, Arc<SourceInfo>, Arc<Camera>);

/// A thumbnail to make.
pub struct Job {
    pub path: PathBuf,
    /// Which edit it is ([`omalux_engine::sidecar::export_key`]).
    pub key: String,
    pub settings: Arc<DevelopSettings>,
    pub process: u32,
    /// None for a raw that's to be decoded.
    pub source: Option<Source>,
}

/// A thumbnail made: the frame, the edit it's of, and the picture, in sRGB.
pub type Made = (PathBuf, String, Rgba8);

pub struct Developer {
    ask: Sender<Job>,
    made: Receiver<Option<Made>>,
    /// Asked for, and not given back yet.
    asked: usize,
}

impl Developer {
    /// `cache` is where they're kept; `wake` is called when one's made.
    pub fn new(decoder: Decoder, cache: Option<PathBuf>, wake: impl Fn() + Send + 'static) -> Self {
        let (ask, asked) = channel::<Job>();
        let (give, made) = channel();
        std::thread::Builder::new()
            .name("thumbnails".into())
            .spawn(move || {
                let mut jobs: Vec<Job> = Vec::new();
                loop {
                    if jobs.is_empty() {
                        let Ok(job) = asked.recv() else { return };
                        jobs.push(job);
                    }
                    jobs.extend(asked.try_iter());
                    let Some(job) = jobs.pop() else { continue };
                    // Those replaced are given back, so they're not waited for.
                    let replaced = jobs.iter().filter(|other| other.path == job.path).count();
                    jobs.retain(|other| other.path != job.path);
                    let thumbnail = make(decoder, cache.as_deref(), &job);
                    if let Err(e) = &thumbnail {
                        log::debug!("no developed thumbnail for {}: {e}", job.path.display());
                    }
                    let given = std::iter::repeat_n(None, replaced).chain([thumbnail.ok().map(|image| (job.path, job.key, image))]);
                    for made in given {
                        if give.send(made).is_err() {
                            return;
                        }
                    }
                    wake();
                }
            })
            .expect("spawn thumbnails thread");
        Self { ask, made, asked: 0 }
    }

    pub fn ask(&mut self, job: Job) {
        if self.ask.send(job).is_ok() {
            self.asked += 1;
        }
    }

    #[cfg(test)]
    pub fn busy(&self) -> bool {
        self.asked > 0
    }

    /// The thumbnails made since this was last asked.
    pub fn made(&mut self) -> Vec<Made> {
        let made: Vec<_> = self.made.try_iter().collect();
        self.asked = self.asked.saturating_sub(made.len());
        made.into_iter().flatten().collect()
    }
}

/// `job`'s thumbnail: from the cache, or else rendered and cached.
fn make(decoder: Decoder, cache: Option<&Path>, job: &Job) -> Result<Rgba8, String> {
    let cached = cache.and_then(|dir| thumbs::cached_developed(dir, &job.path, &job.key, job.process).ok());
    if let Some(thumbnail) = cached.as_deref().and_then(|path| thumbs::read(path).ok()) {
        return Ok(thumbnail);
    }
    let thumbnail = match &job.source {
        Some((image, info, camera)) => thumbs::developed(image, info, camera, &job.settings),
        None => {
            let bytes = std::fs::read(&job.path).map_err(|e| e.to_string())?;
            let loaded = (decoder.source)(&bytes, (Profile::of(&job.settings), job.process))?;
            thumbs::developed(&loaded.image, &loaded.info, &loaded.camera, &job.settings)
        }
    };
    if let Some(path) = cached
        && let Err(e) = thumbs::store(&path, &thumbnail)
    {
        log::warn!("couldn't cache a thumbnail at {}: {e}", path.display());
    }
    Ok(thumbnail)
}
