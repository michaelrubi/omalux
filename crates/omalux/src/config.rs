//! The few settings OmaLux has (`~/.config/omalux/config.toml`): where exports
//! go, and the program Ctrl+E opens them in. Shortcuts are in `hotkeys.toml`.
//!
//! Copied from Omacull's `crates/omacull/src/config.rs`.
//!
//! ```toml
//! export_folder = "omalux"
//! editor = "omapix"
//! ```

use serde::Deserialize;

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// The folder inside the shoot that exports go to.
    pub export_folder: String,
    /// The program Ctrl+E opens exported frames in.
    pub editor: String,
}

impl Default for Config {
    fn default() -> Self {
        Self { export_folder: "omalux".into(), editor: "omapix".into() }
    }
}

impl Config {
    /// What `text` sets, with the defaults for the rest and for anything
    /// that can't be used, and what was wrong with it.
    pub fn parse(text: &str) -> (Self, Vec<String>) {
        let mut config: Self = match toml::from_str(text) {
            Ok(config) => config,
            Err(e) => return (Self::default(), vec![format!("Config: failed to parse config.toml: {e}")]),
        };
        let mut warnings = Vec::new();
        if config.export_folder.trim().is_empty() {
            warnings.push("Config: export_folder is empty".into());
            config.export_folder = Self::default().export_folder;
        }
        if config.editor.trim().is_empty() {
            warnings.push("Config: editor is empty".into());
            config.editor = Self::default().editor;
        }
        (config, warnings)
    }

    /// Read `config.toml` from the config folder, writing one that lists
    /// every setting, commented out, if there's none. Returns warnings
    /// about problems in it.
    pub fn load() -> (Self, Vec<String>) {
        let Some(path) = crate::hotkeys::config_dir().map(|dir| dir.join("config.toml")) else {
            return (Self::default(), Vec::new());
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let written = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(&path, TEMPLATE));
                if let Err(e) = written {
                    log::warn!("Config: couldn't write {}: {e}", path.display());
                }
                (Self::default(), Vec::new())
            }
            Err(e) => (Self::default(), vec![format!("Config: failed to read config.toml: {e}")]),
        }
    }
}

const TEMPLATE: &str = "\
# OmaLux's settings. Each is listed with its default, which OmaLux uses
# unless you change it here: remove the # at the start of its line and edit
# the value. Restart OmaLux to apply. Problems are shown in the status bar
# at startup. Shortcuts are in hotkeys.toml.

# Where exports go (Ctrl+E, Ctrl+Shift+E): a folder inside the shoot's,
# made when it's first needed. A frame already exported is never written
# over, since Omapix saves its retouching back into the same file.
# export_folder = \"omalux\"

# The program Ctrl+E opens the exported frames in, as
# `program --round-trip a.tif b.tif`.
# editor = \"omapix\"
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_template_lists_every_default_and_reads_back_as_them() {
        assert_eq!(Config::parse(TEMPLATE), (Config::default(), Vec::new()));
        // Uncommented, every setting reads back as the default it shows.
        let setting = |l: &'static str| {
            l.strip_prefix("# ").filter(|l| l.split_once(" = ").is_some_and(|(name, _)| !name.contains(' ')))
        };
        let uncommented: String = TEMPLATE.lines().map(|l| format!("{}\n", setting(l).unwrap_or(l))).collect();
        assert_eq!(Config::parse(&uncommented), (Config::default(), Vec::new()), "{uncommented}");
        for setting in ["export_folder", "editor"] {
            assert!(uncommented.contains(&format!("\n{setting} = ")), "{setting} missing");
        }
    }

    #[test]
    fn settings_are_read_and_bad_ones_reported() {
        let text = "export_folder = \"exports\"\neditor = \"gimp\"\n";
        let config = Config { export_folder: "exports".into(), editor: "gimp".into() };
        assert_eq!(Config::parse(text), (config, Vec::new()));

        let (config, warnings) = Config::parse("export_folder = \" \"\neditor = \"\"\n");
        assert_eq!(config, Config::default());
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        let (config, warnings) = Config::parse("unknown_setting = true\n");
        assert_eq!(config, Config::default());
        assert!(warnings[0].contains("failed to parse"), "{warnings:?}");
    }
}
