//! Marks on their way to disk: the sidecar written on a thread of their
//! own so a slow disk never holds up the next frame. Marks are written
//! in the order they were made.
//!
//! Omacull's `omacull-engine/src/disk.rs`, cut down: no decision log.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use crate::shoot::Rating;
use crate::sidecar;

/// A mark made, to be written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    pub path: PathBuf,
    pub rating: Rating,
}

/// Something that went wrong on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// The mark didn't reach the sidecar, which holds `on_disk`.
    Sidecar { path: PathBuf, error: String, on_disk: Rating },
}

/// What the disk thread is asked to do.
enum Work {
    Mark(Box<Mark>),
    /// Say when everything before has been written.
    Flush(Sender<()>),
}

pub struct Disk {
    marks: Option<Sender<Work>>,
    problems: Receiver<Problem>,
    thread: Option<JoinHandle<()>>,
}

impl Disk {
    /// Writes to sidecars on a background thread. `wake` is called when
    /// there's a problem to collect.
    pub fn new(wake: impl Fn() + Send + 'static) -> Self {
        let (marks, rx) = channel::<Work>();
        let (tx, problems) = channel();
        let thread = std::thread::Builder::new()
            .name("disk".into())
            .spawn(move || {
                for work in rx {
                    let mark = match work {
                        Work::Mark(mark) => mark,
                        Work::Flush(done) => {
                            let _ = done.send(());
                            continue;
                        }
                    };
                    let written = sidecar::update(&mark.path, |xmp| sidecar::with_rating(xmp, mark.rating));
                    let problem = match written {
                        Err(e) => Some(Problem::Sidecar {
                            path: mark.path.clone(),
                            error: e.to_string(),
                            on_disk: sidecar::read(&mark.path)
                                .ok()
                                .flatten()
                                .and_then(|xmp| sidecar::rating(&xmp).ok().flatten())
                                .unwrap_or(0),
                        }),
                        Ok(()) => None,
                    };
                    if let Some(problem) = problem {
                        log::warn!("{problem:?}");
                        if tx.send(problem).is_err() {
                            return;
                        }
                        wake();
                    }
                }
            })
            .expect("spawn disk thread");
        Self { marks: Some(marks), problems, thread: Some(thread) }
    }

    pub fn write(&self, mark: Mark) {
        if let Some(marks) = &self.marks {
            let _ = marks.send(Work::Mark(Box::new(mark)));
        }
    }

    /// Wait for every mark sent so far to be written.
    pub fn flush(&self) {
        let (done, wait) = channel();
        if let Some(marks) = &self.marks
            && marks.send(Work::Flush(done)).is_ok()
        {
            let _ = wait.recv();
        }
    }

    pub fn problems(&self) -> impl Iterator<Item = Problem> + '_ {
        self.problems.try_iter()
    }

    /// Wait for every mark sent so far to be written. Nothing more can be
    /// written after.
    pub fn finish(&mut self) {
        self.marks = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Disk {
    fn drop(&mut self) {
        self.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use crate::sidecar::REJECT;

    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("omalux-disk-{}-{name}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn raw(&self, i: usize) -> PathBuf {
            let path = self.0.join(format!("DSC{i:05}.ARW"));
            if !path.exists() {
                fs::write(&path, "").unwrap();
            }
            path
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn marks_reach_the_sidecars_in_order() {
        let folder = Folder::new("order");
        let raw1 = folder.raw(1);
        let raw2 = folder.raw(2);
        let mut disk = Disk::new(|| {});
        disk.write(Mark { path: raw1.clone(), rating: 3 });
        disk.write(Mark { path: raw1.clone(), rating: 0 });
        disk.write(Mark { path: raw2.clone(), rating: REJECT });
        disk.flush();
        let r2 = sidecar::read(&raw2).unwrap().and_then(|x| sidecar::rating(&x).unwrap());
        assert_eq!(r2, Some(REJECT), "written by the time flush returns");
        disk.finish();
        assert_eq!(disk.problems().count(), 0);
        let r1 = sidecar::read(&raw1).unwrap().and_then(|x| sidecar::rating(&x).unwrap());
        assert_eq!(r1, Some(0));
        let r2 = sidecar::read(&raw2).unwrap().and_then(|x| sidecar::rating(&x).unwrap());
        assert_eq!(r2, Some(REJECT));
    }

    #[test]
    fn a_mark_that_cant_be_written_comes_back_with_whats_on_disk() {
        let folder = Folder::new("cant-write");
        let raw = folder.raw(1);
        sidecar::update(&raw, |xmp| sidecar::with_rating(xmp, 2)).unwrap();
        let xmp = sidecar::path_for(&raw);
        let text = fs::read_to_string(&xmp).unwrap();
        // A sidecar this can't place a rating in: it has no description.
        fs::write(&xmp, text.replace("rdf:Description", "rdf:Other")).unwrap();
        let (wake_tx, wake_rx) = channel();
        let mut disk = Disk::new(move || {
            let _ = wake_tx.send(());
        });
        disk.write(Mark { path: raw.clone(), rating: 5 });
        disk.finish();
        assert!(wake_rx.try_recv().is_ok());
        let problems: Vec<_> = disk.problems().collect();
        assert!(
            matches!(&problems[..], [Problem::Sidecar { path, on_disk: 0, .. }] if *path == raw),
            "{problems:?}"
        );
    }

    #[test]
    fn a_sidecar_from_testdata_keeps_every_other_byte_when_it_goes_through_the_disk_thread() {
        const DARKTABLE: &str = include_str!("../testdata/darktable.ARW.xmp");
        const MARKED: &str = include_str!("../testdata/darktable-marked.ARW.xmp");
        let folder = Folder::new("testdata");
        let raw = folder.raw(1);
        fs::write(sidecar::path_for(&raw), DARKTABLE).unwrap();
        let mut disk = Disk::new(|| {});
        disk.write(Mark { path: raw.clone(), rating: 3 });
        disk.finish();
        assert_eq!(disk.problems().count(), 0);
        let on_disk = fs::read_to_string(sidecar::path_for(&raw)).unwrap();
        assert_eq!(on_disk, MARKED);
    }
}
