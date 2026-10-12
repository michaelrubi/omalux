//! A raw's sidecar as OmaLux reads it, and OmaLux's writes to it, to check
//! by hand against Omacull and darktable:
//!
//!     cargo run -p omalux-engine --example sidecar -- DSC01234.ARW
//!     cargo run -p omalux-engine --example sidecar -- DSC01234.ARW --rating 3
//!     cargo run -p omalux-engine --example sidecar -- DSC01234.ARW --exposure 0.5
//!
//! `--rating` marks it as Omacull does (-1 rejects, 0 clears); `--exposure`
//! writes an edit with that Exposure and the rest at their defaults.

use std::path::PathBuf;

use lightcraft_develop::DevelopSettings;
use omalux_engine::sidecar::{self, Edit};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let usage = "usage: sidecar <raw> [--rating N | --exposure EV]";
    let raw = PathBuf::from(args.next().ok_or(usage)?);
    match (args.next().as_deref(), args.next()) {
        (None, _) => {}
        (Some("--rating"), Some(n)) => sidecar::update(&raw, |xmp| sidecar::with_rating(xmp, n.parse().map_err(std::io::Error::other)?))?,
        (Some("--exposure"), Some(ev)) => {
            let mut settings = DevelopSettings::default();
            settings.light.exposure = ev.parse()?;
            sidecar::update(&raw, |xmp| sidecar::with_edit(xmp, &Edit::new(settings)))?;
        }
        _ => return Err(usage.into()),
    }
    let path = sidecar::path_for(&raw);
    match sidecar::read(&raw)? {
        None => println!("{}: none", path.display()),
        Some(xmp) => {
            let edit = sidecar::edit(&xmp)?;
            let edit = edit.map(|e| format!("process {}, exposure {:+.2}", e.process, e.settings.light.exposure));
            println!("{}: rating {:?}, edit {}", path.display(), sidecar::rating(&xmp)?, edit.as_deref().unwrap_or("none"));
        }
    }
    Ok(())
}
