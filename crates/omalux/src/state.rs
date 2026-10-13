//! What OmaLux remembers between sessions (`~/.config/omalux/state.toml`):
//! the shoots opened recently and where each was left.
//!
//! Omacull's `state.rs`, cut down: only the recent list and where each
//! folder was left, with none of its other switches.

use std::path::{Path, PathBuf};

use omalux_engine::shoot::Filter;

const MAX_RECENT: usize = 10;
/// Folders whose place is remembered, the most recently left first.
const MAX_PLACES: usize = 200;

/// Where a shoot was left.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Place {
    pub dir: PathBuf,
    /// The name of the raw the cursor was on.
    pub frame: String,
    #[serde(default)]
    pub filter: Filter,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct State {
    /// Most recent first.
    #[serde(default)]
    pub recent: Vec<PathBuf>,
    /// Most recent first. Last in the file, as TOML wants tables after
    /// plain values.
    #[serde(default)]
    pub places: Vec<Place>,
    /// Where it's saved; nowhere in tests.
    #[serde(skip)]
    path: Option<PathBuf>,
}

impl State {
    /// Read from `path`, and save there from now on. Folders that have gone
    /// are forgotten.
    pub fn load_from(path: PathBuf) -> Self {
        let mut state: Self = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| toml::from_str(&text).map_err(|e| log::warn!("ignoring {}: {e}", path.display())).ok())
            .unwrap_or_default();
        state.recent.retain(|p| p.is_dir());
        state.recent.truncate(MAX_RECENT);
        state.path = Some(path);
        state
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        let written = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(path, toml::to_string(self).unwrap_or_default()));
        if let Err(e) = written {
            log::warn!("couldn't save {}: {e}", path.display());
        }
    }

    pub fn add_recent(&mut self, dir: &Path) {
        let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        self.recent.retain(|p| *p != dir);
        self.recent.insert(0, dir);
        self.recent.truncate(MAX_RECENT);
        self.save();
    }

    pub fn remove_recent(&mut self, dir: &Path) {
        self.recent.retain(|p| *p != dir);
        self.save();
    }

    /// Where `dir` was left, if it's remembered.
    pub fn place(&self, dir: &Path) -> Option<&Place> {
        let canonical = std::fs::canonicalize(dir).ok();
        self.places.iter().find(|p| {
            p.dir == dir
                || canonical.as_ref().is_some_and(|c| *c == p.dir || std::fs::canonicalize(&p.dir).ok().as_ref() == Some(c))
        })
    }

    pub fn remember(&mut self, place: Place) {
        if self.place(&place.dir) == Some(&place) && self.places.first() == Some(&place) {
            return;
        }
        self.places.retain(|p| p.dir != place.dir);
        self.places.insert(0, place);
        self.places.truncate(MAX_PLACES);
        self.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempFolder(PathBuf);

    impl TempFolder {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("omalux-state-test-{name}-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempFolder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn recent_shoots_and_where_each_was_left_are_remembered() {
        let folder = TempFolder::new("state");
        let (a, b) = (folder.0.join("a"), folder.0.join("b"));
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        let path = folder.0.join("config").join("state.toml");

        let mut state = State::load_from(path.clone());
        assert_eq!(state, State { path: Some(path.clone()), ..State::default() });
        state.add_recent(&a);
        state.add_recent(&b);
        state.add_recent(&a);

        let (a, b) = (std::fs::canonicalize(a).unwrap(), std::fs::canonicalize(b).unwrap());
        let loaded = State::load_from(path.clone());
        assert_eq!(loaded.recent, [a.clone(), b.clone()], "most recent first, once each");

        // Where each folder was left, and the filter it had.
        let mut state = loaded;
        let place = |dir: &Path, frame: &str, filter| Place {
            dir: dir.to_path_buf(),
            frame: frame.into(),
            filter,
        };
        state.remember(place(&a, "DSC00012.ARW", Filter::Picks));
        state.remember(place(&b, "DSC00002.ARW", Filter::All));
        state.remember(place(&a, "DSC00013.ARW", Filter::All));
        let loaded = State::load_from(path.clone());
        assert_eq!(loaded.places.len(), 2);
        assert_eq!(loaded.place(&a), Some(&place(&a, "DSC00013.ARW", Filter::All)));
        assert_eq!(loaded.places[1], place(&b, "DSC00002.ARW", Filter::All));

        std::fs::remove_dir(&b).unwrap();
        assert_eq!(State::load_from(path).recent, [a], "gone folders are forgotten");
    }
}
