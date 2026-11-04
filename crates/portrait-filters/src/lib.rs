//! Generic host for third-party portrait retouch packages.
//!
//! A package is a directory with `manifest.json` and `model.onnx`. This crate scans a folder,
//! decides which Filter › Portrait retouch items are available, and (behind the `onnx` feature)
//! runs the graph. It does not ship trained weights. The contract is `docs/PORTRAIT_FILTERS.md`.
//!
//! PhotoCraft's own retouch *tools* (healing brush, dodge, burn) stay in `photocraft-engine`.
//! This crate is only the external-package host.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod image;
mod manifest;
mod paths;
mod scan;
mod task;

#[cfg(all(feature = "onnx", not(target_arch = "wasm32")))]
mod onnx_run;

pub use image::RgbImage;
pub use manifest::{Layout, TensorContract};
pub use paths::{Platform, default_directory, default_directory_for, resolve_directory};
pub use scan::{Catalog, Package, Skip, scan};
pub use task::{DODGE_BURN_OPACITY, LayerPlacement, MenuWhich, STACK_ORDER, Task, disable_reason, placement};

/// Whether this build can execute `model.onnx`. False for wasm and for native builds that were
/// not compiled with `--features onnx` (engine feature `portrait-onnx`).
pub const fn runtime_available() -> bool {
    cfg!(all(feature = "onnx", not(target_arch = "wasm32")))
}

/// Run `package`'s graph on `image` and return RGB in the same dimensions.
///
/// Without the `onnx` feature (and always on wasm) this returns [`Error::RuntimeUnavailable`]
/// and does not read the model.
pub fn infer(package: &Package, image: &RgbImage) -> Result<RgbImage, Error> {
    if !runtime_available() {
        return Err(Error::RuntimeUnavailable);
    }
    #[cfg(all(feature = "onnx", not(target_arch = "wasm32")))]
    {
        onnx_run::infer(package, image)
    }
    #[cfg(not(all(feature = "onnx", not(target_arch = "wasm32"))))]
    {
        let _ = (package, image);
        Err(Error::RuntimeUnavailable)
    }
}

/// Failures a caller can show. Nothing here panics on a bad package or a bad image.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("portrait filter package: {0}")]
    Package(String),
    #[error("portrait filter model: {0}")]
    Model(String),
    #[error("this build does not include ONNX Runtime; rebuild with --features portrait-onnx")]
    RuntimeUnavailable,
    #[error("image is too large for a portrait filter")]
    TooLarge,
    #[error("portrait filters need an RGB image")]
    NotRgb,
}
