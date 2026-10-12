//! What a Sony raw's maker note says the camera did to its own JPEG: the
//! Creative Style and the Dynamic Range Optimizer. The camera match has a
//! profile per style (Standard and Vivid) and puts DRO's shadow lift back
//! at the level the frame had (see "The look" in DESIGN.md); Lightcraft
//! reads only whether DRO was on.
//!
//! The tags, and what their values mean, are as ExifTool's Sony tag
//! documentation lists them: `0xb020` CreativeStyle (text) and `0xb025`
//! DynamicRangeOptimizer. No code was consulted. Read on the ILCE-7M3 only
//! so far; other Sony bodies may differ (other cameras are on the
//! roadmap's "Later" list).

use lightcraft_tiff::{Tiff, makernote, tags};

const CREATIVE_STYLE: u16 = 0xb020;
const DYNAMIC_RANGE_OPTIMIZER: u16 = 0xb025;

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

/// What the maker note records, as far as it does.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    /// "Standard", "Vivid", "Neutral" and so on, as the camera writes it.
    pub creative_style: Option<String>,
    pub dro: Option<Dro>,
    /// The DRO tag's value when it isn't one of the documented ones.
    pub dro_raw: Option<u64>,
}

/// The Creative Style and DRO a Sony raw (`bytes`, the whole file) was
/// shot with. Empty for anything else.
pub fn settings(bytes: &[u8]) -> Settings {
    read(bytes).unwrap_or_default()
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
    Some(Settings { creative_style, dro, dro_raw: raw.filter(|_| dro.is_none()) })
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

    #[test]
    fn style_and_dro_are_read_from_the_maker_note() {
        for order in [ByteOrder::Little, ByteOrder::Big] {
            let s = settings(&sony_file(order, "SONY", "Vivid", 3));
            assert_eq!(s, Settings { creative_style: Some("Vivid".into()), dro: Some(Dro::Auto), dro_raw: None }, "{order:?}");
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
    fn other_files_say_nothing() {
        assert_eq!(settings(&sony_file(ByteOrder::Little, "NIKON CORPORATION", "Vivid", 3)), Settings::default());
        assert_eq!(settings(b""), Settings::default());
        assert_eq!(settings(b"II*\0\x08\0\0\0\0\0"), Settings::default());
        let mut truncated = sony_file(ByteOrder::Little, "SONY", "Vivid", 3);
        truncated.truncate(truncated.len() - 20);
        let _ = settings(&truncated);
    }
}
