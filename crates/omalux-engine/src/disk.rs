//! Marks and edits on their way to disk: the sidecars written on a thread
//! of their own so a slow disk never holds up the next frame, in the
//! order they were made. One thread for both, so a frame's mark and its
//! edit never write its sidecar at once.
//!
//! Omacull's `omacull-engine/src/disk.rs`, cut down: no decision log.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use crate::shoot::Rating;
use crate::sidecar::{self, Edit, Exported};

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
    /// A frame's edit didn't reach its sidecar.
    Edit { path: PathBuf, error: String },
}

/// What the disk thread is asked to do.
enum Work {
    Mark(Box<Mark>),
    /// A raw's edit, as it stands.
    Edit(PathBuf, Box<Edit>),
    /// A raw's last export.
    Exported(PathBuf, Box<Exported>),
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
                    let problem = match work {
                        Work::Mark(mark) => sidecar::update(&mark.path, |xmp| sidecar::with_rating(xmp, mark.rating)).err().map(|e| {
                            let on_disk = sidecar::read(&mark.path).ok().flatten().and_then(|xmp| sidecar::rating(&xmp).ok().flatten());
                            Problem::Sidecar { path: mark.path.clone(), error: e.to_string(), on_disk: on_disk.unwrap_or(0) }
                        }),
                        Work::Edit(path, edit) => {
                            let written = sidecar::update(&path, |xmp| sidecar::with_edit(xmp, &edit));
                            written.err().map(|e| Problem::Edit { path, error: e.to_string() })
                        }
                        Work::Exported(path, exported) => {
                            let written = sidecar::update(&path, |xmp| sidecar::with_exported(xmp, &exported));
                            written.err().map(|e| Problem::Edit { path, error: e.to_string() })
                        }
                        Work::Flush(done) => {
                            let _ = done.send(());
                            continue;
                        }
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

    /// Write a raw's edit to its sidecar, after everything sent before.
    pub fn save(&self, path: PathBuf, edit: Edit) {
        if let Some(marks) = &self.marks {
            let _ = marks.send(Work::Edit(path, Box::new(edit)));
        }
    }

    /// Keep a raw's last export in its sidecar, after everything sent
    /// before.
    pub fn exported(&self, path: PathBuf, exported: Exported) {
        if let Some(marks) = &self.marks {
            let _ = marks.send(Work::Exported(path, Box::new(exported)));
        }
    }

    /// Wait for everything sent so far to be written.
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
    fn edits_and_marks_of_one_frame_are_written_in_turn() {
        use lightcraft_develop::DevelopSettings;
        let folder = Folder::new("edits");
        let raw = folder.raw(1);
        let mut disk = Disk::new(|| {});
        let mut settings = DevelopSettings::for_raw(5200.0, 3.0);
        for step in 1..=20 {
            settings.light.exposure = f64::from(step) / 10.0;
            disk.save(raw.clone(), Edit::new(settings.clone()));
            disk.write(Mark { path: raw.clone(), rating: step % 6 });
        }
        disk.finish();
        assert_eq!(disk.problems().count(), 0);
        let xmp = sidecar::read(&raw).unwrap().unwrap();
        assert_eq!(sidecar::rating(&xmp).unwrap(), Some(2));
        assert_eq!(sidecar::edit(&xmp).unwrap().map(|edit| edit.settings), Some(settings));

        // An edit that can't be written says so.
        fs::write(sidecar::path_for(&raw), "not xmp").unwrap();
        let mut disk = Disk::new(|| {});
        disk.save(raw.clone(), Edit::new(DevelopSettings::default()));
        disk.finish();
        assert!(matches!(&disk.problems().collect::<Vec<_>>()[..], [Problem::Edit { path, .. }] if *path == raw));
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
