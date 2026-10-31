//! A raw's XMP sidecar: the one Omacull writes its marks to, where OmaLux
//! keeps its edit too.
//!
//! OmaLux owns two things in it, and every other byte stays as it was:
//!
//! - **The edit**, in the `omalux:` namespace: Lightcraft's
//!   `DevelopSettings` as JSON in `omalux:settings`, beside
//!   `omalux:processVersion`. Written with Lightcraft's `merge_xmp`, which
//!   removes OmaLux's old properties and writes the new ones into a
//!   description of their own just before `</rdf:RDF>`.
//! - **The last export**, in the same namespace (`omalux:exportedFile`,
//!   `omalux:exportedSettings`, `omalux:exportedModified`): the TIFF, the
//!   edit it was rendered from, and when the TIFF was last written, so a
//!   frame edited since can be told and exported again ([`Exported`]).
//!   Kept when the edit is written, and the edit when it is.
//! - **The rating** (`xmp:Rating`, -1 rejected), written as Omacull writes
//!   it: the characters of its value replaced and nothing else, so either
//!   tool can change a mark. (`merge_xmp` would move it into OmaLux's
//!   description, which Omacull reads too but darktable's history wouldn't
//!   be the only thing left where it was.)
//!
//! Which file: `DSC01234.ARW.xmp` (darktable's naming, Omacull's default),
//! or `DSC01234.xmp` (Omacull's `sidecar = "adobe"`) if that's the one that
//! exists. A new one is named darktable's way.
//!
//! The rating code is Omacull's `sidecar.rs`, copied.

use std::fs;
use std::io::{self, ErrorKind, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};

use lightcraft_develop::DevelopSettings;
use lightcraft_meta::{MergeRules, merge_xmp, parse_xmp};
use quick_xml::events::Event;
use quick_xml::name::{QName, ResolveResult};
use quick_xml::reader::NsReader;

/// `xmp:Rating` as darktable and Omacull read it: -1 rejected, 0
/// unrated, 1 to 5 stars.
pub const REJECT: i32 = -1;

/// How this OmaLux renders an edit (Lightroom's process version). Bumped
/// whenever the same settings would render differently (a re-sync of
/// Lightcraft that changes a reference render, a changed stage), so an
/// edit made before renders as it did, or says it can't.
///
/// 1: M1 to M3. A frame's look is fitted to its own JPEG.
/// 2: M4. The profile is a setting (Camera match with DRO's lift, or
/// Flat), and there's HDR. An edit from 1 has neither, and keeps its
/// fitted look ([`crate::look::Profile::Fitted`]): it renders as it did.
/// 3: Lightcraft re-synced to `0da998f2`. The look fitted to a frame's
/// JPEG and DRO's lift are measured through the lens warp the camera
/// applied to its JPEG (`c854e172`), and lensfun's profile is scaled to
/// the frame's crop factor in an in-camera crop mode (`4ddbb5a3`). An edit
/// keeps the version it was made with: one from 2 renders as it did.
pub const PROCESS_VERSION: u32 = 3;

/// The `omalux:` namespace.
pub const NAMESPACE: &str = "https://github.com/michaelrubi/omalux/xmp/1.0/";

const XMP_NS: &str = "http://ns.adobe.com/xap/1.0/";
const RDF_NS: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";

/// What OmaLux owns of a sidecar for `merge_xmp`.
const OWNED: MergeRules = MergeRules { owned: &["omalux:*"], owned_if_present: &[] };

/// A frame's edit, as kept in its sidecar.
#[derive(Clone, Debug, PartialEq)]
pub struct Edit {
    /// The process version it was made with.
    pub process: u32,
    pub settings: DevelopSettings,
}

impl Edit {
    /// An edit made with this OmaLux.
    pub fn new(settings: DevelopSettings) -> Self {
        Self { process: PROCESS_VERSION, settings }
    }
}

/// A frame's last export, as kept in its sidecar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exported {
    /// The TIFF's name, in the shoot's export folder.
    pub file: String,
    /// The edit it was rendered from ([`export_key`]).
    pub settings: String,
    /// When the TIFF was last written, in seconds since 1970: when it's
    /// still that, nothing (Omapix) has saved into it since.
    pub modified: u64,
}

/// What names an edit for [`Exported::settings`]: its settings' hash, or
/// `fresh` for a frame with none.
pub fn export_key(settings: Option<&DevelopSettings>) -> String {
    settings.map_or_else(|| "fresh".to_owned(), |settings| format!("{:016x}", settings.hash64()))
}

fn invalid(what: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, what)
}

/// A raw's sidecar: the one that's there, darktable's or Adobe's naming,
/// darktable's if neither is.
pub fn path_for(raw: &Path) -> PathBuf {
    let mut darktable = raw.as_os_str().to_owned();
    darktable.push(".xmp");
    let darktable = PathBuf::from(darktable);
    let adobe = raw.with_extension("xmp");
    if !darktable.exists() && adobe.exists() { adobe } else { darktable }
}

/// The edit in a sidecar's text, if it has one. An edit made with a newer
/// OmaLux (a later process version) is refused: this one would render it
/// differently.
pub fn edit(xmp: &str) -> io::Result<Option<Edit>> {
    let data = parse_xmp(xmp).map_err(|e| invalid(e.to_string()))?;
    let text = |name: &str| data.values.get(name).and_then(|v| v.text()).map(str::to_owned);
    let Some(settings) = text("omalux:settings") else {
        return Ok(None);
    };
    let process = match text("omalux:processVersion") {
        Some(v) => v.parse().map_err(|_| invalid(format!("sidecar has a process version that isn't one: {v:?}")))?,
        None => return Err(invalid("sidecar has an edit without a process version")),
    };
    if process > PROCESS_VERSION {
        return Err(invalid(format!("the edit was made with a newer OmaLux (process version {process})")));
    }
    let json: serde_json::Value = serde_json::from_str(&settings).map_err(|e| invalid(format!("sidecar's edit isn't JSON: {e}")))?;
    let settings = DevelopSettings::from_json(&json).map_err(|e| invalid(format!("sidecar's edit can't be read: {e}")))?;
    Ok(Some(Edit { process, settings }))
}

/// The last export in a sidecar's text, if it has one whole.
pub fn exported(xmp: &str) -> io::Result<Option<Exported>> {
    let data = parse_xmp(xmp).map_err(|e| invalid(e.to_string()))?;
    let text = |name: &str| data.values.get(name).and_then(|v| v.text()).map(str::to_owned);
    let (Some(file), Some(settings), Some(modified)) = (text("omalux:exportedFile"), text("omalux:exportedSettings"), text("omalux:exportedModified")) else {
        return Ok(None);
    };
    let modified = modified.parse().map_err(|_| invalid(format!("sidecar has an export time that isn't one: {modified:?}")))?;
    Ok(Some(Exported { file, settings, modified }))
}

/// `<`, `>` and `&` escaped, for element content (quotes needn't be, which
/// keeps the JSON readable).
fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// OmaLux's properties as they're written, in order: (name, value).
type Owned = Vec<(&'static str, String)>;

/// The edit's properties.
fn edit_properties(edit: &Edit) -> io::Result<Owned> {
    let settings = serde_json::to_string(&edit.settings.to_json()).map_err(invalid)?;
    Ok(vec![("processVersion", edit.process.to_string()), ("settings", settings)])
}

/// The export's properties.
fn export_properties(exported: &Exported) -> Owned {
    vec![("exportedFile", exported.file.clone()), ("exportedSettings", exported.settings.clone()), ("exportedModified", exported.modified.to_string())]
}

/// `xmp` with OmaLux's properties replaced by `properties`, and every
/// byte that isn't OmaLux's as it was.
fn with_owned(xmp: &str, properties: &Owned) -> io::Result<String> {
    let elements: String = properties.iter().map(|(name, value)| format!("   <omalux:{name}>{}</omalux:{name}>\n", escape(value))).collect();
    let fresh = format!(
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n \
         <rdf:RDF xmlns:rdf=\"{RDF_NS}\">\n  \
         <rdf:Description rdf:about=\"\"\n    \
         xmlns:omalux=\"{NAMESPACE}\">\n\
         {elements}  \
         </rdf:Description>\n \
         </rdf:RDF>\n\
         </x:xmpmeta>\n",
    );
    merge_xmp(xmp, &fresh, OWNED).map_err(|e| invalid(e.to_string()))
}

/// A sidecar's text with `edit` in it, its last export kept, and every
/// byte that isn't OmaLux's as it was.
pub fn with_edit(xmp: &str, edit: &Edit) -> io::Result<String> {
    let mut properties = edit_properties(edit)?;
    // An export record that can't be read is dropped with the rest of OmaLux's.
    if let Ok(Some(exported)) = exported(xmp) {
        properties.extend(export_properties(&exported));
    }
    with_owned(xmp, &properties)
}

/// A sidecar's text with `exported` as its last export, its edit kept as
/// it is (whatever OmaLux made it), and every byte that isn't OmaLux's as
/// it was.
pub fn with_exported(xmp: &str, exported: &Exported) -> io::Result<String> {
    let data = parse_xmp(xmp).map_err(|e| invalid(e.to_string()))?;
    let text = |name: &str| data.values.get(&format!("omalux:{name}")).and_then(|v| v.text()).map(str::to_owned);
    let mut properties: Owned = ["processVersion", "settings"].into_iter().filter_map(|name| Some((name, text(name)?))).collect();
    properties.extend(export_properties(exported));
    with_owned(xmp, &properties)
}

/// A sidecar for a raw that has none, with nothing in it yet.
pub fn empty() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"OmaLux\">\n \
         <rdf:RDF xmlns:rdf=\"{RDF_NS}\">\n  \
         <rdf:Description rdf:about=\"\"\n    \
         xmlns:xmp=\"{XMP_NS}\"/>\n \
         </rdf:RDF>\n\
         </x:xmpmeta>\n"
    )
}

/// Where the rating is in a sidecar, or where it would go.
enum Place {
    /// The characters of the value.
    Value(Range<usize>),
    /// No rating: an attribute can go at `at`, in the first
    /// `rdf:Description` tag. `declared` if `xmp:` means XMP there.
    Missing { at: usize, declared: bool },
}

/// Where `part`, a slice of `xmp` handed out by the reader, sits in it.
fn span(xmp: &str, part: &str) -> io::Result<Range<usize>> {
    (part.as_ptr() as usize)
        .checked_sub(xmp.as_ptr() as usize)
        .map(|start| start..start + part.len())
        .filter(|range| range.end <= xmp.len())
        .ok_or_else(|| invalid("sidecar has escapes where the rating is"))
}

fn locate(xmp: &str) -> io::Result<Place> {
    let is = |result: &ResolveResult, namespace: &str| matches!(result, ResolveResult::Bound(n) if n.0 == namespace.as_bytes());
    let mut reader = NsReader::from_str(xmp);
    let mut missing = None;
    loop {
        let (namespace, event) = reader.read_resolved_event().map_err(invalid)?;
        let (in_rdf, in_xmp) = (is(&namespace, RDF_NS), is(&namespace, XMP_NS));
        match event {
            Event::Start(e) | Event::Empty(e) if in_rdf && e.local_name().as_ref() == b"Description" => {
                for attribute in e.attributes() {
                    let attribute = attribute.map_err(invalid)?;
                    let (namespace, name) = reader.resolver().resolve_attribute(attribute.key);
                    if is(&namespace, XMP_NS) && name.as_ref() == b"Rating" {
                        let value = std::str::from_utf8(&attribute.value).map_err(invalid)?;
                        return span(xmp, value).map(Place::Value);
                    }
                }
                if missing.is_none() {
                    let name = std::str::from_utf8(e.name().0).map_err(invalid)?;
                    let at = span(xmp, name)?.end;
                    let declared = match reader.resolver().resolve_attribute(QName(b"xmp:Rating")).0 {
                        ResolveResult::Bound(n) if n.0 == XMP_NS.as_bytes() => true,
                        ResolveResult::Unknown(_) => false,
                        _ => return Err(invalid("sidecar uses the xmp: prefix for something else")),
                    };
                    missing = Some(Place::Missing { at, declared });
                }
            }
            // Other writers spell it <xmp:Rating>3</xmp:Rating>.
            Event::Start(e) if in_xmp && e.local_name().as_ref() == b"Rating" => {
                return match reader.read_event().map_err(invalid)? {
                    Event::Text(text) => {
                        let text = std::str::from_utf8(text.as_ref()).map_err(invalid)?;
                        span(xmp, text).map(Place::Value)
                    }
                    _ => Err(invalid("sidecar has a rating that isn't a number")),
                };
            }
            Event::Empty(e) if in_xmp && e.local_name().as_ref() == b"Rating" => {
                return Err(invalid("sidecar has an empty rating"));
            }
            Event::Eof => return missing.ok_or_else(|| invalid("sidecar has no rdf:Description")),
            _ => {}
        }
    }
}

/// The rating in a sidecar's text, if it has one.
pub fn rating(xmp: &str) -> io::Result<Option<i32>> {
    match locate(xmp)? {
        Place::Value(range) => xmp[range].trim().parse().map(Some).map_err(invalid),
        Place::Missing { .. } => Ok(None),
    }
}

/// A sidecar's text with its rating changed and every other byte as it was.
pub fn with_rating(xmp: &str, rating: i32) -> io::Result<String> {
    if !(REJECT..=5).contains(&rating) {
        return Err(io::Error::new(ErrorKind::InvalidInput, "rating must be -1 to 5"));
    }
    let (range, value) = match locate(xmp)? {
        Place::Value(range) => (range, rating.to_string()),
        Place::Missing { at, declared } => {
            let namespace = if declared { "" } else { " xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\"" };
            (at..at, format!("{namespace} xmp:Rating=\"{rating}\""))
        }
    };
    Ok([&xmp[..range.start], &value, &xmp[range.end..]].concat())
}

/// A raw's sidecar's text, or `None` if it has none.
pub fn read(raw: &Path) -> io::Result<Option<String>> {
    match fs::read_to_string(path_for(raw)) {
        Ok(xmp) => Ok(Some(xmp)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Change a raw's sidecar with `change`, making one if there's none. The
/// sidecar is replaced in one step (a temporary file, synced, renamed over
/// it), so it's never half written; an unchanged one isn't touched.
pub fn update(raw: &Path, change: impl FnOnce(&str) -> io::Result<String>) -> io::Result<()> {
    let path = path_for(raw);
    let (old, permissions) = match fs::read_to_string(&path) {
        Ok(old) => {
            let permissions = fs::metadata(&path)?.permissions();
            (Some(old), Some(permissions))
        }
        Err(e) if e.kind() == ErrorKind::NotFound => (None, None),
        Err(e) => return Err(e),
    };
    let new = change(old.as_deref().unwrap_or(&empty()))?;
    if old.as_deref() == Some(new.as_str()) {
        return Ok(());
    }
    let mut name = path.clone().into_os_string();
    name.push(".omalux-tmp");
    let temporary = PathBuf::from(name);
    let written = (|| {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(new.as_bytes())?;
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)?;
        }
        file.sync_all()?;
        fs::rename(&temporary, &path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sidecar darktable 5.6 wrote, with a 14-step history (Omacull's
    /// test file).
    const DARKTABLE: &str = include_str!("../testdata/darktable.ARW.xmp");
    /// A sidecar Omacull wrote for a raw that had none: a pick.
    const OMACULL: &str = include_str!("../testdata/omacull.ARW.xmp");
    /// darktable's after Omacull marked it 3 stars.
    const MARKED: &str = include_str!("../testdata/darktable-marked.ARW.xmp");

    /// A folder of its own for a test, removed when dropped.
    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("omalux-sidecar-{}-{name}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn edited() -> Edit {
        let mut settings = DevelopSettings::for_raw(6500.0, 0.0);
        settings.light.exposure = 0.75;
        settings.light.highlights = -40.0;
        Edit::new(settings)
    }

    /// `after` is `before` with only `inserted` put in at one place.
    fn only_inserted(before: &str, after: &str) -> String {
        let start = before.bytes().zip(after.bytes()).take_while(|(a, b)| a == b).count();
        let end = before.len() - before.bytes().rev().zip(after.bytes().rev()).take_while(|(a, b)| a == b).count();
        let end = end.max(start);
        assert_eq!(start, end, "something of the old file changed");
        after[start..after.len() - (before.len() - end)].to_owned()
    }

    #[test]
    fn an_edit_goes_in_without_touching_anything_else() {
        for (name, before) in [("darktable", DARKTABLE), ("omacull", OMACULL), ("marked", MARKED)] {
            let after = with_edit(before, &edited()).unwrap();
            let inserted = only_inserted(before, &after);
            assert!(inserted.contains("<omalux:settings>") && inserted.contains("omalux:processVersion"), "{name}: {inserted}");
            assert_eq!(edit(&after).unwrap(), Some(edited()), "{name}");
            // The rating is where it was, and Omacull's reading of it the same.
            assert_eq!(rating(&after).unwrap(), rating(before).unwrap(), "{name}");
        }
    }

    #[test]
    fn a_changed_edit_replaces_the_old_one_and_none_accumulate() {
        let once = with_edit(DARKTABLE, &edited()).unwrap();
        let mut other = edited();
        other.settings.light.exposure = -1.0;
        let twice = with_edit(&once, &other).unwrap();
        assert_eq!(edit(&twice).unwrap(), Some(other));
        assert_eq!(twice.matches("<omalux:settings>").count(), 1);
        assert_eq!(twice.len(), once.len() + "-1.0".len() - "0.75".len());
        // The same edit again is the same file.
        assert_eq!(with_edit(&once, &edited()).unwrap(), once);
    }

    #[test]
    fn a_mark_and_an_edit_dont_disturb_each_other() {
        let edited_first = with_rating(&with_edit(DARKTABLE, &edited()).unwrap(), 3).unwrap();
        let marked_first = with_edit(&with_rating(DARKTABLE, 3).unwrap(), &edited()).unwrap();
        assert_eq!(edited_first, marked_first);
        assert_eq!(rating(&edited_first).unwrap(), Some(3));
        assert_eq!(edit(&edited_first).unwrap(), Some(edited()));
        // OmaLux's mark is the one Omacull would have made.
        assert_eq!(with_rating(DARKTABLE, 3).unwrap(), MARKED);
        // A pick in a sidecar that only had OmaLux's edit, as Omacull adds one.
        let fresh = with_edit(&empty(), &edited()).unwrap();
        assert_eq!(rating(&fresh).unwrap(), None);
        let picked = with_rating(&fresh, 1).unwrap();
        assert_eq!(rating(&picked).unwrap(), Some(1));
        assert_eq!(edit(&picked).unwrap(), Some(edited()));
    }

    #[test]
    fn an_export_and_an_edit_keep_each_other() {
        let record = Exported { file: "DSC01234.tif".into(), settings: export_key(Some(&edited().settings)), modified: 1_760_000_000 };
        for (name, before) in [("darktable", DARKTABLE), ("omacull", OMACULL), ("marked", MARKED)] {
            assert_eq!(exported(before).unwrap(), None, "{name}");
            // Exported before it was edited: the record alone goes in.
            let after = with_exported(before, &record).unwrap();
            assert!(only_inserted(before, &after).contains("<omalux:exportedFile>DSC01234.tif"), "{name}");
            assert_eq!((exported(&after).unwrap(), edit(&after).unwrap()), (Some(record.clone()), None), "{name}");
            // Edited since: the record stays.
            let edited_after = with_edit(&after, &edited()).unwrap();
            assert_eq!((exported(&edited_after).unwrap(), edit(&edited_after).unwrap()), (Some(record.clone()), Some(edited())), "{name}");
            // Exported again: the edit stays, the record is the new one, once.
            let again = Exported { modified: record.modified + 60, ..record.clone() };
            let twice = with_exported(&edited_after, &again).unwrap();
            assert_eq!((exported(&twice).unwrap(), edit(&twice).unwrap()), (Some(again), Some(edited())), "{name}");
            assert_eq!(twice.matches("omalux:exportedFile>").count(), 2, "{name}: one element, opened and closed");
            assert_eq!(rating(&twice).unwrap(), rating(before).unwrap(), "{name}");
            assert!(only_inserted(before, &twice).contains("omalux:"), "{name}: nothing else changed");
        }
        assert_eq!(export_key(None), "fresh");
        assert_ne!(export_key(Some(&edited().settings)), export_key(Some(&DevelopSettings::default())));
    }

    #[test]
    fn no_edit_is_none_and_a_bad_one_is_an_error() {
        assert_eq!(edit(DARKTABLE).unwrap(), None);
        assert_eq!(edit(&empty()).unwrap(), None);
        let after = with_edit(DARKTABLE, &edited()).unwrap();
        let version = format!(">{PROCESS_VERSION}</omalux:processVersion>");
        let newer = after.replace(&version, ">99</omalux:processVersion>");
        assert!(edit(&newer).unwrap_err().to_string().contains("newer OmaLux"));
        // One made before is read, and is what it was.
        let older = edit(&after.replace(&version, ">1</omalux:processVersion>")).unwrap().unwrap();
        assert_eq!((older.process, older.settings), (1, edited().settings));
        let broken = after.replace("<omalux:settings>{", "<omalux:settings>[");
        assert!(edit(&broken).is_err());
        assert!(with_edit("not xml at all", &edited()).is_err());
    }

    #[test]
    fn markup_in_the_json_is_escaped() {
        assert_eq!(escape("a<b>&c\"d\""), "a&lt;b&gt;&amp;c\"d\"");
    }

    #[test]
    fn the_sidecar_there_is_the_one_used() {
        let folder = Folder::new("naming");
        let raw = folder.0.join("DSC00001.ARW");
        assert_eq!(path_for(&raw), folder.0.join("DSC00001.ARW.xmp"));
        fs::write(folder.0.join("DSC00001.xmp"), OMACULL).unwrap();
        assert_eq!(path_for(&raw), folder.0.join("DSC00001.xmp"));
        // darktable's wins where there are both, as Omacull's default.
        fs::write(folder.0.join("DSC00001.ARW.xmp"), DARKTABLE).unwrap();
        assert_eq!(path_for(&raw), folder.0.join("DSC00001.ARW.xmp"));
    }

    #[test]
    fn updating_makes_a_sidecar_and_leaves_nothing_behind() {
        let folder = Folder::new("update");
        let raw = folder.0.join("DSC00001.ARW");
        assert_eq!(read(&raw).unwrap(), None);
        update(&raw, |xmp| with_edit(xmp, &edited())).unwrap();
        let xmp = read(&raw).unwrap().unwrap();
        assert_eq!(edit(&xmp).unwrap(), Some(edited()));
        update(&raw, |xmp| with_rating(xmp, REJECT)).unwrap();
        assert_eq!(rating(&read(&raw).unwrap().unwrap()).unwrap(), Some(REJECT));
        assert!(update(&raw, |xmp| with_rating(xmp, 6)).is_err());
        let names: Vec<_> = fs::read_dir(&folder.0).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, ["DSC00001.ARW.xmp"]);

        // darktable's sidecar keeps its history, and isn't touched again by
        // the same edit.
        let raw = folder.0.join("MJR00685.ARW");
        fs::write(path_for(&raw), DARKTABLE).unwrap();
        update(&raw, |xmp| with_edit(xmp, &edited())).unwrap();
        let written = read(&raw).unwrap().unwrap();
        only_inserted(DARKTABLE, &written);
        let before = fs::metadata(path_for(&raw)).unwrap().modified().unwrap();
        update(&raw, |xmp| with_edit(xmp, &edited())).unwrap();
        assert_eq!(fs::metadata(path_for(&raw)).unwrap().modified().unwrap(), before);
    }
}
