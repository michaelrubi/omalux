//! What OmaLux needs from a Sony ARW: its capture time and embedded thumbnail.
//!
//! Ported from Omacull's raw reader (`~/dev/omacull/crates/omacull-engine/src/raw.rs`),
//! cut down to Sony ARW.
//!
//! An ARW is a TIFF. Only the directories are read, a few hundred bytes each,
//! so opening a 50 MB raw costs a handful of small reads.

use std::fs::File;
use std::io::{self, ErrorKind};
use std::os::unix::fs::FileExt;
use std::path::Path;

use lightcraft_raster::Rgba8;

/// A JPEG the camera embedded in the raw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Embedded {
    pub offset: u64,
    pub len: u64,
}

pub struct RawFile {
    file: File,
    pub jpegs: Vec<Embedded>,
    /// When the shutter fired, as Exif writes it (`2026:10:04 15:43:16`),
    /// with the fraction of a second after a dot if the camera recorded it.
    pub captured: Option<String>,
}

const JPEG_OFFSET: u16 = 0x0201;
const JPEG_LENGTH: u16 = 0x0202;
const SUB_IFDS: u16 = 0x014a;
const EXIF_IFD: u16 = 0x8769;
const DATE_TIME_ORIGINAL: u16 = 0x9003;
const SUB_SEC_TIME_ORIGINAL: u16 = 0x9291;

fn invalid(what: &str) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, what)
}

struct Entry {
    tag: u16,
    kind: u16,
    count: u32,
    value: [u8; 4],
}

struct Tiff<'a> {
    file: &'a File,
    big_endian: bool,
    base: u64,
    first: u64,
}

#[derive(Default)]
struct Found {
    jpegs: Vec<Embedded>,
    date: Option<String>,
    fraction: Option<String>,
}

impl<'a> Tiff<'a> {
    fn at(file: &'a File, base: u64) -> io::Result<Self> {
        let mut header = [0; 8];
        file.read_exact_at(&mut header, base)?;
        let big_endian = match &header[..2] {
            b"II" => false,
            b"MM" => true,
            _ => return Err(invalid("not a TIFF")),
        };
        let mut tiff = Self { file, big_endian, base, first: 0 };
        if tiff.u16(&header[2..]) != 42 {
            return Err(invalid("not a TIFF"));
        }
        tiff.first = u64::from(tiff.u32(&header[4..]));
        Ok(tiff)
    }

    fn read(&self, bytes: &mut [u8], offset: u64) -> io::Result<()> {
        self.file.read_exact_at(bytes, self.base + offset)
    }

    fn walk(&self, found: &mut Found) -> io::Result<()> {
        let mut pending = vec![self.first];
        let mut seen = 0;
        while let Some(offset) = pending.pop() {
            seen += 1;
            if offset == 0 || seen > 32 {
                continue;
            }
            let (entries, next) = self.directory(offset)?;
            pending.push(next);
            let (mut at, mut len) = (None, None);
            for e in &entries {
                match e.tag {
                    JPEG_OFFSET => at = Some(self.number(e)),
                    JPEG_LENGTH => len = Some(self.number(e)),
                    SUB_IFDS => pending.extend(self.offsets(e)?),
                    EXIF_IFD => pending.push(u64::from(self.u32(&e.value))),
                    DATE_TIME_ORIGINAL if found.date.is_none() => found.date = self.text(e).ok(),
                    SUB_SEC_TIME_ORIGINAL if found.fraction.is_none() => found.fraction = self.text(e).ok(),
                    _ => {}
                }
            }
            if let (Some(at), Some(len)) = (at, len)
                && len > 0
            {
                found.jpegs.push(Embedded { offset: self.base + u64::from(at), len: u64::from(len) });
            }
        }
        Ok(())
    }

    fn u16(&self, b: &[u8]) -> u16 {
        let b = [b[0], b[1]];
        if self.big_endian { u16::from_be_bytes(b) } else { u16::from_le_bytes(b) }
    }

    fn u32(&self, b: &[u8]) -> u32 {
        let b = [b[0], b[1], b[2], b[3]];
        if self.big_endian { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) }
    }

    fn directory(&self, offset: u64) -> io::Result<(Vec<Entry>, u64)> {
        let mut count = [0; 2];
        self.read(&mut count, offset)?;
        let count = usize::from(self.u16(&count));
        let mut bytes = vec![0; count * 12 + 4];
        self.read(&mut bytes, offset + 2)?;
        let entries = bytes[..count * 12]
            .as_chunks::<12>()
            .0
            .iter()
            .map(|e| Entry {
                tag: self.u16(e),
                kind: self.u16(&e[2..]),
                count: self.u32(&e[4..]),
                value: [e[8], e[9], e[10], e[11]],
            })
            .collect();
        Ok((entries, u64::from(self.u32(&bytes[count * 12..]))))
    }

    fn number(&self, e: &Entry) -> u32 {
        if e.kind == 3 { u32::from(self.u16(&e.value)) } else { self.u32(&e.value) }
    }

    fn text(&self, e: &Entry) -> io::Result<String> {
        let mut bytes = vec![0; e.count.min(64) as usize];
        if e.count <= 4 {
            let len = bytes.len();
            bytes.copy_from_slice(&e.value[..len]);
        } else {
            self.read(&mut bytes, u64::from(self.u32(&e.value)))?;
        }
        Ok(String::from_utf8_lossy(&bytes).trim_end_matches('\0').trim().to_owned())
    }

    fn offsets(&self, e: &Entry) -> io::Result<Vec<u64>> {
        if e.count <= 1 {
            return Ok(vec![u64::from(self.u32(&e.value))]);
        }
        let mut bytes = vec![0; e.count.min(16) as usize * 4];
        self.read(&mut bytes, u64::from(self.u32(&e.value)))?;
        Ok(bytes.as_chunks::<4>().0.iter().map(|b| u64::from(self.u32(b))).collect())
    }
}

impl RawFile {
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        let mut found = Found::default();
        Tiff::at(&file, 0)?.walk(&mut found)?;
        let captured = found.date.filter(|d| d.starts_with(|c: char| c.is_ascii_digit())).map(|date| match found.fraction {
            Some(f) if !f.is_empty() => format!("{date}.{f}"),
            _ => date,
        });
        Ok(Self { file, jpegs: found.jpegs, captured })
    }

    /// The smallest embedded JPEG: the camera's own thumbnail.
    pub fn thumbnail(&self) -> Option<Embedded> {
        self.jpegs.iter().copied().min_by_key(|j| j.len)
    }

    pub fn read(&self, jpeg: Embedded) -> io::Result<Vec<u8>> {
        let mut bytes = vec![0; jpeg.len as usize];
        self.file.read_exact_at(&mut bytes, jpeg.offset)?;
        Ok(bytes)
    }
}

/// Decode a thumbnail JPEG to sRGB pixels, matching `thumbs::decode_thumb`.
pub fn decode_thumb(jpeg: &[u8]) -> Option<Rgba8> {
    let decoded = lightcraft_codecs::decode(jpeg, lightcraft_codecs::DecodeOptions::default()).ok()?;
    Some(decoded.to_srgb8())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thumbs::testing::{FakeArw, TestFolder};

    #[test]
    fn reads_capture_time_and_thumbnail_from_fake_arw() {
        let folder = TestFolder::new("arw-read");
        let raw_path = folder.raw(1);
        FakeArw::default().write_with_captured(&raw_path, "2026:10:04 15:43:16.123");

        let raw = RawFile::open(&raw_path).unwrap();
        assert_eq!(raw.captured.as_deref(), Some("2026:10:04 15:43:16.123"));
        let thumb = raw.thumbnail().unwrap();
        let bytes = raw.read(thumb).unwrap();
        assert!(!bytes.is_empty());
        let image = decode_thumb(&bytes).unwrap();
        assert_eq!((image.width, image.height), (48, 32));
    }

    #[test]
    fn handles_file_without_capture_time() {
        let folder = TestFolder::new("arw-no-time");
        let raw_path = folder.raw(1);
        let arw = FakeArw::default();
        arw.write(&raw_path);

        let raw = RawFile::open(&raw_path).unwrap();
        assert_eq!(raw.captured, None);
        assert!(raw.thumbnail().is_some());
    }

    #[test]
    fn handles_invalid_or_truncated_files() {
        let folder = TestFolder::new("arw-invalid");
        let path = folder.raw(1);
        std::fs::write(&path, b"not a tiff").unwrap();
        assert!(RawFile::open(&path).is_err());
        std::fs::write(&path, b"").unwrap();
        assert!(RawFile::open(&path).is_err());
    }

    #[test]
    fn reads_corpus_a7m3_if_present() {
        for (name, bytes) in crate::raw::tests::a7m3_corpus() {
            let folder = TestFolder::new("arw-corpus");
            let path = folder.raw(1);
            std::fs::write(&path, bytes).unwrap();
            let raw = RawFile::open(&path).expect(&name);
            assert!(raw.captured.is_some(), "{name} has capture time");
            assert!(raw.thumbnail().is_some(), "{name} has thumbnail");
        }
    }
}
