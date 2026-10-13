//! OmaLux's engine: everything but the window. No UI or GPU dependencies,
//! so it's tested headless. The develop engine underneath is Lightcraft's,
//! vendored (`vendor/lightcraft/`); the modules lifted from Lightcraft's
//! own engine say so.

// Lightcraft allows these, and the modules lifted from it keep its ways.
#![allow(clippy::type_complexity)]

pub mod arw;
pub mod auto;
pub mod camera_preview;
pub mod camera_profiles;
pub mod color;
pub mod compare;
pub mod copy;
pub mod crop;
pub mod disk;
pub mod edit;
pub mod export;
pub mod lens_profiles;
pub mod raw;
#[cfg(test)]
mod reference;
pub mod shoot;
pub mod sidecar;
pub mod sony;
pub mod stacks;
pub mod thumbs;
