//! An open shoot: a folder's raws and their marks, which one is current,
//! and which the filter shows.
//!
//! This is only the model. Omacull's `cull.rs`, cut down to what a
//! developer needs of it: the picks are what's shown, and a filter shows
//! everything.

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::sidecar::{self, REJECT};

/// Sony's raws: the ILCE-7M3 is the only camera so far (other cameras are
/// on the roadmap's "Later" list).
pub const RAW_EXTENSIONS: &[&str] = &["arw"];

/// A mark as Omacull and darktable store it in `xmp:Rating`: -1 rejected,
/// 0 not yet decided, 1 a pick (one star), up to 5 stars.
pub type Rating = i32;

pub const PICK: Rating = 1;

/// A mark changing on one frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub index: usize,
    pub was: Rating,
    pub now: Rating,
}

/// "★★★" for three stars; a pick is one star, and "✕" a reject.
pub fn stars(rating: Rating) -> String {
    match rating {
        REJECT => "✕".to_owned(),
        stars if stars > 0 => "★".repeat(stars as usize),
        _ => String::new(),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub path: PathBuf,
    pub rating: Rating,
}

impl Frame {
    /// The raw's file name, e.g. `DSC01234.ARW`.
    pub fn name(&self) -> String {
        self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }
}

/// Which frames the filmstrip shows and stepping visits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Filter {
    /// Rated 1 or more: what Omacull left to develop.
    #[default]
    Picks,
    All,
}

impl Filter {
    pub fn matches(self, frame: &Frame) -> bool {
        match self {
            Filter::Picks => frame.rating >= PICK,
            Filter::All => true,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Filter::Picks => "Picks",
            Filter::All => "All",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Previous,
    Next,
    First,
    Last,
}

pub struct Shoot {
    dir: PathBuf,
    frames: Vec<Frame>,
    current: usize,
    filter: Filter,
    /// 1 or -1: the way the cursor last moved, for preparing frames ahead.
    direction: isize,
    /// Frames picked out in the filmstrip, for paste and export.
    selected: BTreeSet<usize>,
}

fn raw(path: &Path) -> bool {
    let hidden = path.file_name().is_some_and(|n| n.as_encoded_bytes().starts_with(b"."));
    let extension = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    !hidden && extension.is_some_and(|e| RAW_EXTENSIONS.contains(&e.as_str())) && path.is_file()
}

impl Shoot {
    /// The raws in a folder, in name order, with the marks their sidecars
    /// hold, showing the picks; a folder with no picks yet is opened on
    /// all of it. Also returns the sidecars that couldn't be read, which
    /// count as unmarked.
    pub fn open(dir: &Path) -> io::Result<(Self, Vec<String>)> {
        let mut paths: Vec<PathBuf> = fs::read_dir(dir)?.filter_map(Result::ok).map(|e| e.path()).filter(|p| raw(p)).collect();
        if paths.is_empty() {
            return Err(io::Error::new(ErrorKind::NotFound, format!("no raws in {}", dir.display())));
        }
        paths.sort();
        let read: Vec<(Frame, Option<String>)> = paths
            .into_par_iter()
            .map(|path| {
                let rating = sidecar::read(&path).and_then(|xmp| xmp.as_deref().map(sidecar::rating).transpose());
                let (rating, problem) = match rating {
                    Ok(rating) => (rating.flatten().unwrap_or(0).clamp(REJECT, 5), None),
                    Err(e) => (0, Some(format!("{}: {e}", sidecar::path_for(&path).display()))),
                };
                (Frame { path, rating }, problem)
            })
            .collect();
        let (frames, problems): (Vec<Frame>, Vec<Option<String>>) = read.into_iter().unzip();
        Ok((Self::new(dir.to_path_buf(), frames), problems.into_iter().flatten().collect()))
    }

    /// A shoot of these frames, on its first pick, or on all of it if it
    /// has none.
    pub fn new(dir: PathBuf, frames: Vec<Frame>) -> Self {
        let first = frames.iter().position(|f| Filter::Picks.matches(f));
        let filter = if first.is_some() { Filter::Picks } else { Filter::All };
        Self { dir, frames, current: first.unwrap_or(0), filter, direction: 1, selected: BTreeSet::new() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }

    pub fn current(&self) -> usize {
        self.current
    }

    pub fn frame(&self) -> &Frame {
        &self.frames[self.current]
    }

    pub fn filter(&self) -> Filter {
        self.filter
    }

    pub fn direction(&self) -> isize {
        self.direction
    }

    fn matches(&self, index: usize) -> bool {
        self.filter.matches(&self.frames[index])
    }

    /// Shown in the filmstrip: what the filter lets through, and the
    /// current frame, which stays until the cursor leaves it even if a
    /// mark took it out of the filter.
    pub fn shown(&self, index: usize) -> bool {
        index == self.current || self.matches(index)
    }

    pub fn shown_indices(&self) -> Vec<usize> {
        (0..self.frames.len()).filter(|&i| self.shown(i)).collect()
    }

    /// The frames the filter lets through after `from` going `direction`.
    fn onward(&self, from: usize, direction: isize) -> impl Iterator<Item = usize> + '_ {
        let indices: Box<dyn Iterator<Item = usize>> =
            if direction > 0 { Box::new(from + 1..self.frames.len()) } else { Box::new((0..from).rev()) };
        indices.filter(|&i| self.matches(i))
    }

    /// The next `ahead` frames stepping would visit, the way the cursor
    /// last moved; and the `behind` it would visit going back.
    pub fn neighbours(&self, ahead: usize, behind: usize) -> Vec<usize> {
        let mut near: Vec<usize> = self.onward(self.current, self.direction).take(ahead).collect();
        near.extend(self.onward(self.current, -self.direction).take(behind));
        near
    }

    pub fn go_to(&mut self, index: usize) {
        if index < self.frames.len() && index != self.current {
            self.direction = if index > self.current { 1 } else { -1 };
            self.current = index;
        }
    }

    /// Go to a raw by its path, showing everything if the filter would
    /// leave it out. False if it isn't in the shoot.
    pub fn go_to_path(&mut self, path: &Path) -> bool {
        let Some(index) = self.frames.iter().position(|f| f.path == path) else { return false };
        if !self.matches(index) {
            self.filter = Filter::All;
        }
        self.go_to(index);
        true
    }

    /// Move the cursor among the shown frames, leaving the selection as it is.
    fn move_step(&mut self, step: Step) -> bool {
        let to = match step {
            Step::Next => self.onward(self.current, 1).next(),
            Step::Previous => self.onward(self.current, -1).next(),
            Step::First => (0..self.frames.len()).find(|&i| self.matches(i)),
            Step::Last => (0..self.frames.len()).rev().find(|&i| self.matches(i)),
        };
        let moved = to.is_some_and(|to| to != self.current);
        if let Some(to) = to {
            self.go_to(to);
        }
        moved
    }

    /// Move the cursor among the shown frames. False if it can't go further.
    /// Clears the selection, as in Lightroom.
    pub fn step(&mut self, step: Step) -> bool {
        self.clear_selection();
        self.move_step(step)
    }

    /// Show only what `filter` lets through. If that leaves out the current
    /// frame, the cursor moves to the nearest frame that's in, next first.
    /// Clears the selection.
    pub fn set_filter(&mut self, filter: Filter) {
        self.clear_selection();
        self.filter = filter;
        if !filter.matches(self.frame()) {
            let to = self.onward(self.current, 1).next().or_else(|| self.onward(self.current, -1).next());
            if let Some(to) = to {
                self.current = to;
            }
        }
    }

    /// The frames picked out in the filmstrip, in order: what a paste or an
    /// export reaches. None until a frame is picked out; the current frame
    /// alone is no selection. Never a frame a mark has since taken out of
    /// the filter: a paste mustn't reach what isn't shown.
    pub fn selection(&self) -> Vec<usize> {
        self.selected.iter().copied().filter(|&i| i == self.current || self.matches(i)).collect()
    }

    pub fn is_selected(&self, index: usize) -> bool {
        self.selected.contains(&index)
    }

    /// Ctrl+click: add a frame to the selection, or take it out, and go to
    /// it. The current frame is in the selection it starts.
    pub fn toggle_selected(&mut self, index: usize) {
        if self.selected.is_empty() {
            self.selected.insert(self.current);
        }
        if !self.selected.remove(&index) {
            self.selected.insert(index);
        }
        self.go_to(index);
    }

    /// Shift+click: select the shown frames from the current one to `index`,
    /// and go there.
    pub fn select_to(&mut self, index: usize) {
        let (from, to) = (self.current.min(index), self.current.max(index));
        self.selected = (from..=to).filter(|&i| self.shown(i)).collect();
        self.go_to(index);
    }

    /// Shift+arrow: step, taking the frames passed into the selection.
    /// Steps without clearing the selection.
    pub fn extend(&mut self, step: Step) -> bool {
        self.selected.insert(self.current);
        let moved = self.move_step(step);
        self.selected.insert(self.current);
        moved
    }

    /// Ctrl+A: select every shown frame.
    pub fn select_all(&mut self) {
        self.selected = self.shown_indices().into_iter().collect();
    }

    /// Ctrl+D: select nothing.
    pub fn clear_selection(&mut self) {
        self.selected.clear();
    }

    /// Mark the current frame. None if it already had that mark.
    pub fn mark(&mut self, rating: Rating) -> Option<Change> {
        let frame = &mut self.frames[self.current];
        let was = std::mem::replace(&mut frame.rating, rating);
        (was != rating).then_some(Change { index: self.current, was, now: rating })
    }

    /// Put back what a sidecar holds after writing to it failed.
    pub fn restore(&mut self, path: &Path, rating: Rating) {
        if let Some(frame) = self.frames.iter_mut().find(|f| f.path == path) {
            frame.rating = rating;
        }
    }

    /// The stack a frame is in: the frames shot with it that the filter
    /// lets through, itself among them, in order. None for a frame that
    /// isn't in a stack of two or more of those. (M2 skeleton: `m2-stacks`
    /// fills these in.)
    pub fn stack(&self, _index: usize) -> Option<Vec<usize>> {
        None
    }

    /// Open out the current frame's stack in the filmstrip, or close it
    /// up. False if it isn't in one.
    pub fn toggle_stack(&mut self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shoot(ratings: &[Rating]) -> Shoot {
        let frames = ratings.iter().enumerate().map(|(i, &rating)| Frame { path: format!("/shoot/DSC{i:05}.ARW").into(), rating });
        Shoot::new("/shoot".into(), frames.collect())
    }

    #[test]
    fn opening_finds_the_raws_in_name_order_with_their_marks() {
        let dir = std::env::temp_dir().join(format!("omalux-shoot-{}", std::process::id()));
        fs::create_dir_all(dir.join("omalux")).unwrap();
        for name in ["DSC00003.ARW", "DSC00001.ARW", "DSC00002.arw", ".DSC00004.ARW", "DSC00001.JPG", "notes.txt"] {
            fs::write(dir.join(name), "").unwrap();
        }
        let marked = |rating: i32| sidecar::with_rating(&sidecar::empty(), rating).unwrap();
        fs::write(dir.join("DSC00001.ARW.xmp"), marked(3)).unwrap();
        fs::write(dir.join("DSC00002.xmp"), marked(REJECT)).unwrap();
        fs::write(dir.join("DSC00003.ARW.xmp"), "not xmp").unwrap();

        let (shoot, problems) = Shoot::open(&dir).unwrap();
        let found: Vec<(String, Rating)> = shoot.frames().iter().map(|f| (f.name(), f.rating)).collect();
        assert_eq!(found, [("DSC00001.ARW".into(), 3), ("DSC00002.arw".into(), REJECT), ("DSC00003.ARW".into(), 0)]);
        assert!(problems.len() == 1 && problems[0].contains("DSC00003.ARW.xmp"), "{problems:?}");
        assert_eq!((shoot.filter(), shoot.current()), (Filter::Picks, 0));

        assert_eq!(Shoot::open(&dir.join("omalux")).err().map(|e| e.kind()), Some(ErrorKind::NotFound));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_picks_are_shown_and_a_shoot_without_any_is_shown_whole() {
        let mut s = shoot(&[0, 1, REJECT, 3, 0]);
        assert_eq!((s.filter(), s.current(), s.shown_indices()), (Filter::Picks, 1, vec![1, 3]));
        assert!(s.step(Step::Next) && s.current() == 3);
        assert!(!s.step(Step::Next), "the last pick");
        assert!(s.step(Step::First) && s.current() == 1);

        s.set_filter(Filter::All);
        assert_eq!(s.shown_indices(), [0, 1, 2, 3, 4]);
        assert!(s.step(Step::Last) && s.current() == 4);
        // Back to the picks: the cursor moves to the nearest one.
        s.set_filter(Filter::Picks);
        assert_eq!(s.current(), 3);

        let unculled = shoot(&[0, 0, REJECT]);
        assert_eq!((unculled.filter(), unculled.current()), (Filter::All, 0));
    }

    #[test]
    fn neighbours_lie_the_way_the_cursor_is_going() {
        let mut s = shoot(&[1, 1, 1, 1, 1, 1]);
        s.go_to(2);
        assert_eq!(s.neighbours(2, 1), [3, 4, 1]);
        s.step(Step::Previous);
        assert_eq!((s.direction(), s.neighbours(2, 1)), (-1, vec![0, 2]));
    }

    #[test]
    fn a_raw_thats_opened_is_gone_to_whatever_the_filter() {
        let mut s = shoot(&[1, 0, 1]);
        assert!(s.go_to_path(Path::new("/shoot/DSC00001.ARW")));
        assert_eq!((s.current(), s.filter()), (1, Filter::All));
        assert!(!s.go_to_path(Path::new("/shoot/DSC00009.ARW")));
    }

    #[test]
    fn marking_changes_the_current_frames_rating_and_reports_it() {
        let mut s = shoot(&[1, 0]);
        assert_eq!(s.mark(3), Some(Change { index: 0, was: 1, now: 3 }));
        assert_eq!(s.frame().rating, 3);
        assert_eq!(s.mark(3), None, "already 3 stars");
        assert_eq!(s.mark(REJECT), Some(Change { index: 0, was: 3, now: REJECT }));
        assert_eq!(s.frame().rating, REJECT);
    }

    #[test]
    fn restoring_puts_back_the_rating_of_a_path() {
        let mut s = shoot(&[1, 0]);
        s.mark(5);
        s.restore(Path::new("/shoot/DSC00000.ARW"), 1);
        assert_eq!(s.frame().rating, 1);
        s.restore(Path::new("/shoot/nonexistent.ARW"), 3);
        assert_eq!(s.frame().rating, 1);
    }

    #[test]
    fn stars_formats_ratings() {
        assert_eq!(stars(3), "★★★");
        assert_eq!(stars(PICK), "★");
        assert_eq!(stars(0), "");
        assert_eq!(stars(REJECT), "✕");
    }

    #[test]
    fn frames_are_selected_by_ctrl_shift_and_arrows() {
        let mut s = shoot(&[1, 0, 1, 0, 1]);
        s.toggle_selected(2);
        assert_eq!((s.selection(), s.current()), (vec![0, 2], 2), "starting from the current frame");
        s.toggle_selected(0);
        assert_eq!((s.selection(), s.current()), (vec![2], 0));
        s.select_to(4);
        assert_eq!(s.selection(), [0, 2, 4], "only shown frames");
        s.clear_selection();
        s.extend(Step::Previous);
        assert_eq!((s.selection(), s.current()), (vec![2, 4], 2));
        s.select_all();
        assert_eq!(s.selection(), [0, 2, 4]);
    }

    #[test]
    fn a_frame_marked_out_of_the_filter_leaves_the_selection_with_the_cursor() {
        let mut s = shoot(&[1, 1, 1]);
        s.extend(Step::Next);
        s.mark(0);
        assert_eq!(s.selection(), [0, 1], "the current frame stays until it's left");
        s.extend(Step::Next);
        assert_eq!((s.selection(), s.shown_indices()), (vec![0, 2], vec![0, 2]));
    }

    #[test]
    fn a_plain_step_clears_the_selection() {
        let mut s = shoot(&[1, 1, 1]);
        s.select_all();
        assert_eq!(s.selection(), [0, 1, 2]);
        assert!(s.step(Step::Next));
        assert!(s.selection().is_empty());
        assert_eq!(s.current(), 1);

        s.select_all();
        assert_eq!(s.selection(), [0, 1, 2]);
        s.step(Step::First);
        assert!(s.selection().is_empty());
        assert_eq!(s.current(), 0);
    }

    #[test]
    fn extend_at_the_end_of_the_strip_keeps_whats_selected() {
        let mut s = shoot(&[1, 1, 1]);
        s.select_to(2);
        assert_eq!(s.selection(), [0, 1, 2]);
        assert_eq!(s.current(), 2);
        assert!(!s.extend(Step::Next));
        assert_eq!(s.selection(), [0, 1, 2]);
        assert_eq!(s.current(), 2);

        s.go_to(0);
        s.clear_selection();
        s.extend(Step::Next);
        assert_eq!(s.selection(), [0, 1]);
        assert_eq!(s.current(), 1);
        s.step(Step::First);
        s.extend(Step::Next);
        assert_eq!(s.selection(), [0, 1]);
        s.go_to(0);
        assert!(!s.extend(Step::Previous));
        assert_eq!(s.selection(), [0, 1]);
    }

    #[test]
    fn set_filter_clears_the_selection() {
        let mut s = shoot(&[1, 0, 1]);
        s.select_all();
        assert_eq!(s.selection(), [0, 2]);
        s.set_filter(Filter::All);
        assert!(s.selection().is_empty());

        s.select_all();
        assert_eq!(s.selection(), [0, 1, 2]);
        s.set_filter(Filter::Picks);
        assert!(s.selection().is_empty());
    }

    #[test]
    fn select_all_takes_only_the_shown_frames() {
        let mut s = shoot(&[1, 0, 1, REJECT, 1]);
        assert_eq!(s.filter(), Filter::Picks);
        s.select_all();
        assert_eq!(s.selection(), [0, 2, 4]);

        s.set_filter(Filter::All);
        s.select_all();
        assert_eq!(s.selection(), [0, 1, 2, 3, 4]);
    }
}
