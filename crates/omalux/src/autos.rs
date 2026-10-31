//! Raws decoded for what Auto Sync left to be found on them: the frames of
//! a selection the renderer hasn't decoded, which are often far from the
//! ones it prepares. One thread, so a large selection never takes the
//! decoding the loupe is waiting on.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use omalux_engine::raw::Loaded;

use crate::render::{Decoder, Development};

/// A raw decoded, or why it couldn't be.
pub type Found = (PathBuf, Result<Loaded, String>);

pub struct Autos {
    ask: Sender<(PathBuf, Development)>,
    found: Receiver<Found>,
    /// Asked for, and not given back yet.
    asked: HashSet<PathBuf>,
}

impl Autos {
    /// `wake` is called when there's a raw to collect.
    pub fn new(decoder: Decoder, wake: impl Fn() + Send + 'static) -> Self {
        let (ask, asked) = channel::<(PathBuf, Development)>();
        let (give, found) = channel();
        std::thread::Builder::new()
            .name("autos".into())
            .spawn(move || {
                for (path, development) in asked {
                    let loaded = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|bytes| (decoder.source)(&bytes, development));
                    if give.send((path, loaded)).is_err() {
                        return;
                    }
                    wake();
                }
            })
            .expect("spawn autos thread");
        Self { ask, found, asked: HashSet::new() }
    }

    /// Decode `path`, unless that's been asked already.
    pub fn ask(&mut self, path: &Path, development: Development) {
        if self.asked.insert(path.to_path_buf()) {
            let _ = self.ask.send((path.to_path_buf(), development));
        }
    }

    pub fn busy(&self) -> bool {
        !self.asked.is_empty()
    }

    /// The next raw decoded, if one is.
    pub fn found(&mut self) -> Option<Found> {
        let found = self.found.try_recv().ok()?;
        self.asked.remove(&found.0);
        Some(found)
    }

    /// Wait for the next raw decoded. None if there's none to wait for.
    pub fn wait(&mut self) -> Option<Found> {
        if !self.busy() {
            return None;
        }
        let found = self.found.recv().ok()?;
        self.asked.remove(&found.0);
        Some(found)
    }
}
