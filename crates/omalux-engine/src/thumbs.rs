//! Filmstrip thumbnails, cached on disk (`~/.cache/omalux/thumbnails/`).
//!
//! Lifted from Omacull (`omacull-engine/src/thumbs.rs` and `loader.rs`).
//!
//! Thumbnails are made by shrinking the raw's embedded preview to at most
//! [`LONG_EDGE`] pixels, upright, in sRGB. Because decoding the preview
//! requires reading the raw file, thumbnails are cached on disk as small
//! JPEGs named for the raw's path, size and modification time. The cache is
//! disposable; with no cache directory, thumbnails are made every time.
//!
//! Off the UI thread, [`Loader`] loads thumbnails in the order they are
//! wanted, dropping jobs that are no longer wanted whenever the request changes.
//!
//! A frame with an edit gets a developed thumbnail too ([`developed`]),
//! cached under that edit, so going back to an edit finds it there.

use std::collections::{HashSet, VecDeque};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::UNIX_EPOCH;

use lightcraft_develop::DevelopSettings;
use lightcraft_pipeline::{RenderRequest, SourceInfo};
use lightcraft_raster::{Rgb32f, Rgba8};

use crate::look::{self, Camera};

/// The long edge of a thumbnail, in pixels: sharp on a HiDPI filmstrip.
pub const LONG_EDGE: usize = 320;
const QUALITY: u8 = 85;
/// Bumped when thumbnails are made differently, so old ones aren't used.
const VERSION: u64 = 1;
/// [`VERSION`], for developed thumbnails: bumped when a re-sync changes
/// how frames render.
const DEVELOPED: u64 = 1;

/// `$XDG_CACHE_HOME/omalux`, or `~/.cache/omalux`.
pub fn cache_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(dir.join("omalux"))
}

/// `$XDG_CACHE_HOME/omalux/thumbnails`, or `~/.cache/omalux/thumbnails`.
pub fn default_dir() -> Option<PathBuf> {
    Some(cache_dir()?.join("thumbnails"))
}

/// FNV-1a: stable between builds, unlike std's hasher.
pub(crate) fn fnv(bytes: &[u8], mut hash: u64) -> u64 {
    for &b in bytes {
        hash = (hash ^ u64::from(b)).wrapping_mul(0x100_0000_01b3);
    }
    hash
}

/// A name for what's cached of a raw: changes with its path, size and
/// modification time, and with `version`, how it was made.
pub(crate) fn key(raw: &Path, version: u64) -> io::Result<String> {
    let raw = fs::canonicalize(raw)?;
    let meta = fs::metadata(&raw)?;
    let modified = meta.modified()?.duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let mut hash = fnv(raw.as_os_str().as_encoded_bytes(), 0xcbf2_9ce4_8422_2325);
    for part in [meta.len(), modified as u64, (modified >> 64) as u64, version] {
        hash = fnv(&part.to_le_bytes(), hash);
    }
    Ok(format!("{hash:016x}"))
}

/// Where a raw's thumbnail is cached in `dir`.
pub fn cached(dir: &Path, raw: &Path) -> io::Result<PathBuf> {
    Ok(dir.join(format!("{}.jpg", key(raw, VERSION)?)))
}

/// Where a raw's developed thumbnail of edit `edit`
/// ([`crate::sidecar::export_key`]) under process version `process` is
/// cached in `dir`.
pub fn cached_developed(dir: &Path, raw: &Path, edit: &str, process: u32) -> io::Result<PathBuf> {
    Ok(dir.join(format!("{}-{edit}-{process}.jpg", key(raw, DEVELOPED)?)))
}

/// A frame as its edit renders it, no larger than [`LONG_EDGE`], in sRGB.
pub fn developed(source: &Rgb32f, info: &SourceInfo, camera: &Camera, settings: &DevelopSettings) -> Rgba8 {
    let (settings, info) = look::effective(settings, info, camera);
    lightcraft_pipeline::render(source, &info, &settings, &RenderRequest::fit(LONG_EDGE, LONG_EDGE)).image
}

/// A thumbnail kept at `path`.
pub fn read(path: &Path) -> io::Result<Rgba8> {
    decode_thumb(&fs::read(path)?)
}

fn encode_thumb(thumbnail: &Rgba8) -> io::Result<Vec<u8>> {
    let img = lightcraft_codecs::EncodeImage::rgba8(thumbnail);
    lightcraft_codecs::encode_jpeg(
        &img,
        QUALITY,
        lightcraft_codecs::ChromaSubsampling::S420,
        &lightcraft_codecs::EncodeMeta::default(),
    )
    .map_err(|e| io::Error::other(e.to_string()))
}

pub(crate) fn decode_thumb(jpeg: &[u8]) -> io::Result<Rgba8> {
    let decoded = lightcraft_codecs::decode(jpeg, lightcraft_codecs::DecodeOptions::default())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    Ok(decoded.to_srgb8())
}

/// Make a thumbnail from a raw file: shrinking the embedded preview to at most
/// [`LONG_EDGE`], upright and in sRGB.
pub fn make(raw: &Path) -> io::Result<Rgba8> {
    let bytes = fs::read(raw)?;
    crate::raw::embedded_preview_srgb(&bytes, LONG_EDGE)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, format!("no embedded preview in {}", raw.display())))
}

/// Keep a thumbnail, replacing the file in one step so a reader never sees
/// half of it.
pub fn store(path: &Path, thumbnail: &Rgba8) -> io::Result<()> {
    static UNIQUE: AtomicUsize = AtomicUsize::new(0);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{}-{}.tmp", std::process::id(), UNIQUE.fetch_add(1, Ordering::Relaxed)));
    let temporary = PathBuf::from(name);
    let bytes = encode_thumb(thumbnail)?;
    let written = fs::write(&temporary, bytes).and_then(|()| fs::rename(&temporary, path));
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

/// A raw's thumbnail, in sRGB: from the cache in `dir` if it's there, else
/// made and cached. With no cache it's made every time.
pub fn load(raw: &Path, dir: Option<&Path>) -> io::Result<Rgba8> {
    let Some(dir) = dir else { return make(raw) };
    let path = cached(dir, raw)?;
    if let Ok(jpeg) = fs::read(&path)
        && let Ok(thumbnail) = decode_thumb(&jpeg)
    {
        return Ok(thumbnail);
    }
    let thumbnail = make(raw)?;
    if let Err(e) = store(&path, &thumbnail) {
        log::warn!("couldn't cache a thumbnail in {}: {e}", dir.display());
    }
    Ok(thumbnail)
}

/// Make sure a raw's thumbnail is in the cache, without decoding it if it
/// already is.
pub fn warm(raw: &Path, dir: &Path) -> io::Result<()> {
    let path = cached(dir, raw)?;
    if path.exists() {
        return Ok(());
    }
    store(&path, &make(raw)?)
}

/// A thumbnail loaded off the UI thread.
pub struct Loaded {
    pub index: usize,
    pub result: io::Result<Rgba8>,
}

#[derive(Default)]
struct Queue {
    waiting: VecDeque<usize>,
    running: HashSet<usize>,
    stop: bool,
}

struct Shared {
    queue: Mutex<Queue>,
    work: Condvar,
}

impl Shared {
    fn next(&self) -> Option<usize> {
        let mut queue = self.queue.lock().unwrap();
        loop {
            if queue.stop {
                return None;
            }
            if let Some(index) = queue.waiting.pop_front() {
                queue.running.insert(index);
                return Some(index);
            }
            queue = self.work.wait(queue).unwrap();
        }
    }
}

/// Loads filmstrip thumbnails off the UI thread in the order they are wanted.
pub struct Loader {
    shared: Arc<Shared>,
    results: Receiver<Loaded>,
    threads: Vec<JoinHandle<()>>,
}

impl Loader {
    /// A loader for these raws, caching thumbnails in `cache`. `wake` is
    /// called whenever a thumbnail has loaded.
    pub fn new(
        raws: Vec<PathBuf>,
        cache: Option<PathBuf>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let count = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(2, 8);
        Self::with_threads(count, raws, cache, wake)
    }

    /// A loader with a fixed number of worker threads (0 for synchronous tests).
    pub fn with_threads(
        count: usize,
        raws: Vec<PathBuf>,
        cache: Option<PathBuf>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let shared = Arc::new(Shared {
            queue: Mutex::default(),
            work: Condvar::new(),
        });
        let (tx, results) = channel();
        let (raws, cache, wake) = (Arc::new(raws), Arc::new(cache), Arc::new(wake));
        let threads = (0..count)
            .map(|n| {
                let (shared, tx) = (shared.clone(), tx.clone());
                let (raws, cache, wake) = (raws.clone(), cache.clone(), wake.clone());
                std::thread::Builder::new()
                    .name(format!("thumb-loader-{n}"))
                    .spawn(move || {
                        while let Some(index) = shared.next() {
                            let raw = raws.get(index);
                            let result = match raw {
                                Some(path) => load(path, cache.as_deref()),
                                None => Err(io::Error::new(io::ErrorKind::NotFound, "no such frame")),
                            };
                            shared.queue.lock().unwrap().running.remove(&index);
                            if tx.send(Loaded { index, result }).is_err() {
                                return;
                            }
                            wake();
                        }
                    })
                    .expect("spawn thumbnail loader thread")
            })
            .collect();
        Self { shared, results, threads }
    }

    /// Replace what's waiting with `indices`, most wanted first. Jobs already
    /// running aren't started again.
    pub fn want(&self, indices: impl IntoIterator<Item = usize>) {
        let mut queue = self.shared.queue.lock().unwrap();
        let waiting: VecDeque<usize> = indices.into_iter().filter(|i| !queue.running.contains(i)).collect();
        queue.waiting = waiting;
        drop(queue);
        self.shared.work.notify_all();
    }

    /// What has loaded since last checked.
    pub fn loaded(&self) -> impl Iterator<Item = Loaded> + '_ {
        self.results.try_iter()
    }

    /// Whether anything is waiting or running.
    pub fn busy(&self) -> bool {
        let queue = self.shared.queue.lock().unwrap();
        !queue.waiting.is_empty() || !queue.running.is_empty()
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        let mut queue = self.shared.queue.lock().unwrap();
        queue.stop = true;
        queue.waiting.clear();
        drop(queue);
        self.shared.work.notify_all();
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// A fake Sony ARW file with a real embedded JPEG for testing.
    pub struct FakeArw {
        pub width: u16,
        pub height: u16,
        pub rgb: [u8; 3],
        pub orientation: u16,
    }

    impl Default for FakeArw {
        fn default() -> Self {
            Self { width: 48, height: 32, rgb: [200, 120, 40], orientation: 1 }
        }
    }

    impl FakeArw {
        pub fn bytes(&self) -> Vec<u8> {
            self.bytes_with_captured(None)
        }

        pub fn bytes_with_captured(&self, captured: Option<&str>) -> Vec<u8> {
            let px = vec![self.rgb; usize::from(self.width) * usize::from(self.height)];
            let img = lightcraft_codecs::EncodeImage::new(
                u32::from(self.width),
                u32::from(self.height),
                3,
                lightcraft_codecs::Samples::U8(px.as_flattened()),
            );
            let jpeg = lightcraft_codecs::encode_jpeg(
                &img,
                90,
                lightcraft_codecs::ChromaSubsampling::S420,
                &lightcraft_codecs::EncodeMeta::default(),
            )
            .unwrap();

            let mut tiff = Vec::new();
            tiff.extend_from_slice(b"II\x2a\0");
            tiff.extend_from_slice(&8u32.to_le_bytes());

            let (date, frac) = captured.map_or(("", ""), |cap| cap.split_once('.').unwrap_or((cap, "")));
            let has_date = captured.is_some();
            let has_frac = !frac.is_empty();

            let mut entries_count: u16 = 7;
            if has_date {
                entries_count += 1;
            }
            if has_frac {
                entries_count += 1;
            }

            let ifd_len = 2 + usize::from(entries_count) * 12 + 4;
            let data_start = 8 + ifd_len;

            let make_offset = data_start as u32;
            let model_offset = make_offset + 6;
            let date_offset = model_offset + 10;
            let date_len = if has_date { 20 } else { 0 };
            let jpeg_offset = date_offset + date_len;

            tiff.extend_from_slice(&entries_count.to_le_bytes());
            let mut entry = |tag: u16, kind: u16, count: u32, val: u32| {
                tiff.extend_from_slice(&tag.to_le_bytes());
                tiff.extend_from_slice(&kind.to_le_bytes());
                tiff.extend_from_slice(&count.to_le_bytes());
                tiff.extend_from_slice(&val.to_le_bytes());
            };
            entry(0x0100, 4, 1, u32::from(self.width));
            entry(0x0101, 4, 1, u32::from(self.height));
            entry(0x010f, 2, 5, make_offset);
            entry(0x0110, 2, 9, model_offset);
            entry(0x0112, 3, 1, u32::from(self.orientation));
            entry(0x0201, 4, 1, jpeg_offset);
            entry(0x0202, 4, 1, jpeg.len() as u32);
            if has_date {
                entry(0x9003, 2, 20, date_offset);
            }
            if has_frac {
                let mut frac_bytes = [0u8; 4];
                let len = frac.len().min(3);
                frac_bytes[..len].copy_from_slice(&frac.as_bytes()[..len]);
                entry(0x9291, 2, (len + 1) as u32, u32::from_ne_bytes(frac_bytes));
            }

            tiff.extend_from_slice(&0u32.to_le_bytes());

            tiff.extend_from_slice(b"SONY\0\0");
            tiff.extend_from_slice(b"ILCE-7M3\0\0");
            if has_date {
                let mut date_bytes = [0u8; 20];
                let len = date.len().min(19);
                date_bytes[..len].copy_from_slice(&date.as_bytes()[..len]);
                tiff.extend_from_slice(&date_bytes);
            }
            assert_eq!(tiff.len(), jpeg_offset as usize);
            tiff.extend_from_slice(&jpeg);

            tiff
        }

        pub fn write(&self, path: &Path) {
            fs::write(path, self.bytes()).unwrap();
        }

        pub fn write_with_captured(&self, path: &Path, captured: &str) {
            fs::write(path, self.bytes_with_captured(Some(captured))).unwrap();
        }
    }

    pub struct TestFolder(pub PathBuf);

    impl TestFolder {
        pub fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("omalux-test-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        pub fn raw(&self, i: usize) -> PathBuf {
            self.0.join(format!("DSC{i:05}.ARW"))
        }
    }

    impl Drop for TestFolder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn a_developed_thumbnail_is_of_its_edit_with_its_crop_and_cached_under_it() {
        let source = Rgb32f::filled(600, 400, [0.1; 3]);
        let (info, camera) = (SourceInfo::default(), Camera::default());
        let plain = developed(&source, &info, &camera, &DevelopSettings::default());
        assert_eq!((plain.width, plain.height), (LONG_EDGE, LONG_EDGE * 2 / 3));

        let mut edit = DevelopSettings::default();
        edit.light.exposure = 1.0;
        crate::crop::set_aspect(&mut edit, (600.0, 400.0), crate::crop::Aspect::Ratio(1, 1));
        let square = developed(&source, &info, &camera, &edit);
        assert_eq!(square.width, square.height);
        assert!(square.data[0][1] > plain.data[0][1], "brighter: {:?} against {:?}", square.data[0], plain.data[0]);

        let folder = TestFolder::new("thumbs-developed");
        let raw = folder.0.join("DSC00001.ARW");
        fs::write(&raw, b"raw").unwrap();
        let path = cached_developed(&folder.0, &raw, "abc", 3).unwrap();
        assert_ne!(path, cached_developed(&folder.0, &raw, "abd", 3).unwrap(), "another edit");
        assert_ne!(path, cached_developed(&folder.0, &raw, "abc", 2).unwrap(), "another process version");
        store(&path, &square).unwrap();
        let kept = read(&path).unwrap();
        assert_eq!((kept.width, kept.height), (square.width, square.height));
    }

    #[test]
    fn thumbnails_are_cached_and_remade_when_the_raw_changes() {
        let folder = TestFolder::new("thumbs");
        let cache = folder.0.join("cache");
        let raw = folder.raw(1);
        FakeArw { width: 640, height: 480, rgb: [10, 200, 10], orientation: 8 }.write(&raw);

        let first = load(&raw, Some(&cache)).unwrap();
        assert_eq!((first.width, first.height), (240, 320), "shrunk and upright portrait");
        let entries = || fs::read_dir(&cache).unwrap().map(|e| e.unwrap().path()).collect::<Vec<_>>();
        assert_eq!(entries().len(), 1);
        assert!(entries()[0].extension().is_some_and(|e| e == "jpg"));

        // Served from the cache: the raw's preview isn't read again.
        let size = fs::metadata(&raw).unwrap();
        let tiny = Rgba8 { width: 2, height: 2, data: vec![[255, 255, 255, 255]; 4] };
        fs::write(&entries()[0], encode_thumb(&tiny).unwrap()).unwrap();
        let file = fs::File::options().write(true).open(&raw).unwrap();
        file.set_modified(size.modified().unwrap()).unwrap();
        assert_eq!(load(&raw, Some(&cache)).unwrap().width, 2);

        // A different raw at the same path is a different thumbnail.
        FakeArw { width: 64, height: 48, rgb: [10, 200, 10], orientation: 1 }.write(&raw);
        let modified = size.modified().unwrap() + Duration::from_secs(5);
        fs::File::options().write(true).open(&raw).unwrap().set_modified(modified).unwrap();
        assert_eq!(load(&raw, Some(&cache)).unwrap().width, 64);
        assert_eq!(entries().len(), 2);

        // Warming leaves what's there and adds what isn't.
        warm(&raw, &cache).unwrap();
        assert_eq!(entries().len(), 2);
        let other = folder.raw(2);
        FakeArw::default().write(&other);
        warm(&other, &cache).unwrap();
        assert_eq!(entries().len(), 3);
        assert!(entries().iter().all(|p| p.extension().is_some_and(|e| e == "jpg")), "no temporary files left");
    }

    #[test]
    fn without_a_cache_thumbnails_are_still_made() {
        let folder = TestFolder::new("thumbs-uncached");
        let raw = folder.raw(1);
        FakeArw::default().write(&raw);
        assert_eq!(load(&raw, None).unwrap().width, 48);
        assert!(load(&folder.0.join("missing.ARW"), None).is_err());
    }

    #[test]
    fn what_is_no_longer_wanted_is_dropped_and_whats_running_isnt_started_again() {
        let loader = Loader::with_threads(0, Vec::new(), None, || {});
        let waiting = || loader.shared.queue.lock().unwrap().waiting.iter().copied().collect::<Vec<_>>();
        loader.want([3, 1, 2]);
        assert_eq!(waiting(), [3, 1, 2]);
        assert!(loader.busy());
        loader.shared.queue.lock().unwrap().running.insert(2);
        loader.want([2, 3]);
        assert_eq!(waiting(), [3]);
    }

    #[test]
    fn loader_loads_thumbnails_asynchronously() {
        let folder = TestFolder::new("loader-async");
        let raw1 = folder.raw(1);
        let raw2 = folder.raw(2);
        FakeArw { width: 64, height: 48, rgb: [200, 50, 50], orientation: 1 }.write(&raw1);
        FakeArw { width: 48, height: 64, rgb: [50, 200, 50], orientation: 1 }.write(&raw2);

        let wake_count = Arc::new(AtomicUsize::new(0));
        let wake = {
            let count = wake_count.clone();
            move || {
                count.fetch_add(1, Ordering::Relaxed);
            }
        };

        let loader = Loader::with_threads(2, vec![raw1, raw2], Some(folder.0.join("cache")), wake);
        loader.want([0, 1]);

        let start = Instant::now();
        let mut results = Vec::new();
        while results.len() < 2 {
            assert!(start.elapsed() < Duration::from_secs(5), "loader timed out");
            results.extend(loader.loaded());
            std::thread::sleep(Duration::from_millis(5));
        }

        results.sort_by_key(|l| l.index);
        assert_eq!(results[0].index, 0);
        assert_eq!(results[0].result.as_ref().unwrap().width, 64);
        assert_eq!(results[1].index, 1);
        assert_eq!(results[1].result.as_ref().unwrap().height, 64);
        assert!(wake_count.load(Ordering::Relaxed) >= 2);
    }
}
