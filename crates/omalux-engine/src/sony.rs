//! What a Sony raw's maker note says the camera did to its own JPEG: the
//! Creative Style, the Dynamic Range Optimizer, and whether lens compensations
//! (distortion, shading, lateral chromatic aberration) were applied. The camera match
//! has a profile per style (Standard and Vivid) and puts DRO's shadow lift back
//! at the level the frame had (see "The look" in DESIGN.md); Lightcraft
//! reads only whether DRO was on.
//!
//! The tags, and what their values mean, are as ExifTool's Sony tag
//! documentation lists them: `0xb020` CreativeStyle (text), `0xb025`
//! DynamicRangeOptimizer, `0x2011` VignettingCorrection, `0x2012`
//! LateralChromaticAberration, and `0x2013` DistortionCorrectionSetting.
//! No code was consulted. Read on the ILCE-7M3 only so far; other Sony
//! bodies may differ (other cameras are on the roadmap's "Later" list).

use lightcraft_tiff::{Tiff, makernote, tags};

const CREATIVE_STYLE: u16 = 0xb020;
const DYNAMIC_RANGE_OPTIMIZER: u16 = 0xb025;
const VIGNETTING_CORRECTION: u16 = 0x2011;
const LATERAL_CA_CORRECTION: u16 = 0x2012;
const DISTORTION_CORRECTION: u16 = 0x2013;

/// The Dynamic Range Optimizer's setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dro {
    Off,
    /// "Standard", on bodies before the Auto/Lv scheme.
    Standard,
    /// Auto, the factory default: the camera picks the strength per scene.
    Auto,
    /// "Advanced Auto", on older bodies.
    AdvancedAuto,
    /// A fixed level, 1 to 5 ("Advanced Lv" on older bodies too).
    Level(u8),
}

impl Dro {
    fn from_tag(v: u64) -> Option<Dro> {
        Some(match v {
            0 => Dro::Off,
            1 => Dro::Standard,
            2 => Dro::AdvancedAuto,
            3 => Dro::Auto,
            8..=12 => Dro::Level((v - 7) as u8),
            16..=23 => Dro::Level((v - 15) as u8),
            _ => return None,
        })
    }

    pub fn label(self) -> String {
        match self {
            Dro::Off => "Off".into(),
            Dro::Standard => "Standard".into(),
            Dro::Auto => "Auto".into(),
            Dro::AdvancedAuto => "Advanced Auto".into(),
            Dro::Level(n) => format!("Lv{n}"),
        }
    }
}

/// Which lens compensations the camera applied to its own JPEG, as recorded
/// in the maker note (tags `0x2013` DistortionCorrectionSetting, `0x2011`
/// VignettingCorrection, `0x2012` LateralChromaticAberration).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct LensComp {
    pub distortion: bool,
    pub shading: bool,
    pub ca: bool,
}

impl LensComp {
    /// Which corrections were on ("distortion, shading, CA", "shading, CA", "none").
    pub fn label(self) -> String {
        let mut on = Vec::new();
        if self.distortion {
            on.push("distortion");
        }
        if self.shading {
            on.push("shading");
        }
        if self.ca {
            on.push("CA");
        }
        if on.is_empty() {
            "none".into()
        } else {
            on.join(", ")
        }
    }
}

/// What the maker note records, as far as it does.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    /// "Standard", "Vivid", "Neutral" and so on, as the camera writes it.
    pub creative_style: Option<String>,
    pub dro: Option<Dro>,
    /// The DRO tag's value when it isn't one of the documented ones.
    pub dro_raw: Option<u64>,
    pub lens_comp: LensComp,
}

/// The Creative Style, DRO and lens compensations a Sony raw (`bytes`, the
/// whole file) was shot with. Empty for anything else.
pub fn settings(bytes: &[u8]) -> Settings {
    read(bytes).unwrap_or_default()
}

/// Which lens compensations a Sony raw (`bytes`, the whole file) had on.
pub fn lens_comp(bytes: &[u8]) -> LensComp {
    settings(bytes).lens_comp
}

fn read(bytes: &[u8]) -> Option<Settings> {
    let tiff = Tiff::parse(bytes).ok()?;
    let make = tiff.ifds.first()?.string(tags::MAKE)?;
    if !make.trim().to_ascii_uppercase().starts_with("SONY") {
        return None;
    }
    let entry = tiff.exif()?.get(tags::MAKER_NOTE)?;
    let note = makernote::parse_makernote(bytes, entry.offset, entry.count() as u64, tiff.order, &make)?;
    let creative_style = note.ifd.string(CREATIVE_STYLE).map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
    let raw = note.ifd.u64(DYNAMIC_RANGE_OPTIMIZER);
    let dro = raw.and_then(Dro::from_tag);
    let is_on = |tag: u16| note.ifd.u64(tag).is_some_and(|v| v != 0 && v != u32::MAX as u64);
    let lens_comp = LensComp {
        distortion: is_on(DISTORTION_CORRECTION),
        shading: is_on(VIGNETTING_CORRECTION),
        ca: is_on(LATERAL_CA_CORRECTION),
    };
    Some(Settings { creative_style, dro, dro_raw: raw.filter(|_| dro.is_none()), lens_comp })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightcraft_tiff::{ByteOrder, IfdBuilder, TiffWriter, Value};

    /// A Sony-style file: IFD0 → Exif → a maker note `SONY DSC \0\0\0` and
    /// an IFD holding a Creative Style and a DRO value, the layout of the
    /// camera's own notes (offsets relative to the TIFF header).
    fn sony_file(order: ByteOrder, make: &str, style: &str, dro: u32) -> Vec<u8> {
        let (u16b, u32b) = match order {
            ByteOrder::Little => (u16::to_le_bytes as fn(u16) -> [u8; 2], u32::to_le_bytes as fn(u32) -> [u8; 4]),
            ByteOrder::Big => (u16::to_be_bytes as fn(u16) -> [u8; 2], u32::to_be_bytes as fn(u32) -> [u8; 4]),
        };
        let mut text = style.as_bytes().to_vec();
        text.resize(16, 0);
        let mut note = b"SONY DSC \0\0\0".to_vec();
        note.extend(u16b(2));
        // CreativeStyle, ASCII, its text after the IFD: the offset is patched in below
        note.extend(u16b(CREATIVE_STYLE));
        note.extend(u16b(2));
        note.extend(u32b(16));
        let patch = note.len();
        note.extend(u32b(0));
        note.extend(u16b(DYNAMIC_RANGE_OPTIMIZER));
        note.extend(u16b(4));
        note.extend(u32b(1));
        note.extend(u32b(dro));
        note.extend(u32b(0));
        let at = note.len();
        note.extend(&text);
        let mut exif = IfdBuilder::new();
        exif.set(tags::MAKER_NOTE, Value::Undefined(note.clone()));
        let mut ifd0 = IfdBuilder::new();
        ifd0.set(tags::MAKE, Value::Ascii(make.into()));
        ifd0.set(tags::MODEL, Value::Ascii("ILCE-7M3".into()));
        ifd0.set_child(tags::EXIF_IFD, exif);
        let mut file = TiffWriter::new(order, false).write(&[ifd0]).unwrap();
        let start = file.windows(note.len()).position(|w| w == note.as_slice()).unwrap();
        file[start + patch..start + patch + 4].copy_from_slice(&u32b((start + at) as u32));
        file
    }

    fn sony_file_with_comp(order: ByteOrder, make: &str, dist: u32, vig: u32, ca: u32) -> Vec<u8> {
        let (u16b, u32b) = match order {
            ByteOrder::Little => (u16::to_le_bytes as fn(u16) -> [u8; 2], u32::to_le_bytes as fn(u32) -> [u8; 4]),
            ByteOrder::Big => (u16::to_be_bytes as fn(u16) -> [u8; 2], u32::to_be_bytes as fn(u32) -> [u8; 4]),
        };
        let mut note = b"SONY DSC \0\0\0".to_vec();
        note.extend(u16b(3));
        note.extend(u16b(VIGNETTING_CORRECTION));
        note.extend(u16b(4));
        note.extend(u32b(1));
        note.extend(u32b(vig));
        note.extend(u16b(LATERAL_CA_CORRECTION));
        note.extend(u16b(4));
        note.extend(u32b(1));
        note.extend(u32b(ca));
        note.extend(u16b(DISTORTION_CORRECTION));
        note.extend(u16b(4));
        note.extend(u32b(1));
        note.extend(u32b(dist));
        note.extend(u32b(0));
        let mut exif = IfdBuilder::new();
        exif.set(tags::MAKER_NOTE, Value::Undefined(note));
        let mut ifd0 = IfdBuilder::new();
        ifd0.set(tags::MAKE, Value::Ascii(make.into()));
        ifd0.set(tags::MODEL, Value::Ascii("ILCE-7M3".into()));
        ifd0.set_child(tags::EXIF_IFD, exif);
        TiffWriter::new(order, false).write(&[ifd0]).unwrap()
    }

    #[test]
    fn style_and_dro_are_read_from_the_maker_note() {
        for order in [ByteOrder::Little, ByteOrder::Big] {
            let s = settings(&sony_file(order, "SONY", "Vivid", 3));
            assert_eq!(s, Settings { creative_style: Some("Vivid".into()), dro: Some(Dro::Auto), dro_raw: None, lens_comp: LensComp::default() }, "{order:?}");
            let s = settings(&sony_file(order, "SONY", "Standard", 0));
            assert_eq!((s.creative_style.as_deref(), s.dro), (Some("Standard"), Some(Dro::Off)));
        }
        let level = |v| settings(&sony_file(ByteOrder::Little, "SONY", "Standard", v)).dro;
        assert_eq!([16, 18, 20, 8, 12].map(level), [1, 3, 5, 1, 5].map(|n| Some(Dro::Level(n))));
        assert_eq!(level(2), Some(Dro::AdvancedAuto));
        assert_eq!(Dro::Level(4).label(), "Lv4");
        // undocumented values are kept, not guessed at
        let odd = settings(&sony_file(ByteOrder::Little, "SONY", "Standard", 77));
        assert_eq!((odd.dro, odd.dro_raw), (None, Some(77)));
    }

    #[test]
    fn lens_compensation_is_read_from_the_maker_note() {
        for order in [ByteOrder::Little, ByteOrder::Big] {
            let all_on = lens_comp(&sony_file_with_comp(order, "SONY", 2, 2, 2));
            assert_eq!(all_on, LensComp { distortion: true, shading: true, ca: true }, "{order:?}");
            assert_eq!(all_on.label(), "distortion, shading, CA");

            let dist_off = lens_comp(&sony_file_with_comp(order, "SONY", 0, 2, 2));
            assert_eq!(dist_off, LensComp { distortion: false, shading: true, ca: true }, "{order:?}");
            assert_eq!(dist_off.label(), "shading, CA");

            let all_off = lens_comp(&sony_file_with_comp(order, "SONY", 0, 0, 0));
            assert_eq!(all_off, LensComp { distortion: false, shading: false, ca: false }, "{order:?}");
            assert_eq!(all_off.label(), "none");

            let na = lens_comp(&sony_file_with_comp(order, "SONY", u32::MAX, u32::MAX, u32::MAX));
            assert_eq!(na, LensComp::default());
        }
        for (name, bytes) in crate::raw::tests::a7m3_corpus() {
            let s = settings(&bytes);
            assert_eq!(s.lens_comp, LensComp { distortion: false, shading: true, ca: true }, "{name}");
            assert_eq!(s.lens_comp.label(), "shading, CA");
        }
    }

    #[test]
    fn other_files_say_nothing() {
        assert_eq!(settings(&sony_file(ByteOrder::Little, "NIKON CORPORATION", "Vivid", 3)), Settings::default());
        assert_eq!(settings(b""), Settings::default());
        assert_eq!(settings(b"II*\0\x08\0\0\0\0\0"), Settings::default());
        let mut truncated = sony_file(ByteOrder::Little, "SONY", "Vivid", 3);
        truncated.truncate(truncated.len() - 20);
        let _ = settings(&truncated);
    }
}
