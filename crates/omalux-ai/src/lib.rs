//! OmaLux's models, on the system's ONNX Runtime as Omacull's and Omapix's
//! are: CUDA where the library has it (Arch's `onnxruntime-cuda`), the CPU
//! otherwise. For raw AI denoise, once M0's spike has picked the model and
//! whether it runs here or on Lightcraft's own executor (`lightcraft-denoise`).
//!
//! Nothing is downloaded on its own: ONNX Runtime comes from the system
//! (`/usr/lib/libonnxruntime.so`, or `OMALUX_ORT_LIBRARY`).

use std::sync::OnceLock;

/// What went wrong, in words for the status bar.
pub type Result<T> = std::result::Result<T, String>;

/// The ONNX Runtime library to open.
pub fn library() -> String {
    std::env::var("OMALUX_ORT_LIBRARY").unwrap_or_else(|_| "/usr/lib/libonnxruntime.so".into())
}

/// Open ONNX Runtime, once.
pub fn init() -> Result<()> {
    static INIT: OnceLock<Result<()>> = OnceLock::new();
    INIT.get_or_init(|| {
        let library = library();
        let builder = ort::init_from(&library).map_err(|e| format!("Couldn't open ONNX Runtime ({library}): {e}"))?;
        builder.with_name("omalux").commit();
        Ok(())
    })
    .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only where the system has ONNX Runtime; skipped elsewhere.
    #[test]
    fn opens_the_systems_onnx_runtime() {
        if !std::path::Path::new(&library()).exists() {
            eprintln!("skipped: no {}", library());
            return;
        }
        init().unwrap();
    }
}
