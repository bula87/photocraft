//! Fixture graphs only: an identity, a multiply-by-half, and a fixed 4×4 identity.
//! These are not trained portrait weights.

use std::path::PathBuf;

use photocraft_portrait_filters::{RgbImage, infer, runtime_available, scan};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

fn install(folder: &str, task: &str, model: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("pc-portrait-onnx-{folder}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let pkg = root.join(folder);
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("manifest.json"),
        format!(
            r#"{{"format":"photocraft-portrait-filter","formatVersion":1,"task":"{task}","tensor":{{"layout":"nchw","input":"image","output":"image_out"}}}}"#
        ),
    )
    .unwrap();
    std::fs::copy(fixture(model), pkg.join("model.onnx")).unwrap();
    root
}

#[cfg(all(feature = "onnx", not(target_arch = "wasm32")))]
#[test]
fn identity_graph_returns_the_rgb_image() {
    assert!(runtime_available());
    let root = install("heal", "heal", "identity.onnx");
    let package = scan(&root).get("heal").unwrap().clone();
    let mut image = RgbImage::new(2, 2).unwrap();
    image.set(0, 0, [0.25, 0.5, 0.75]);
    image.set(1, 1, [1.0, 0.0, 0.0]);
    let out = infer(&package, &image).unwrap();
    assert_eq!(out, image);
}

#[cfg(all(feature = "onnx", not(target_arch = "wasm32")))]
#[test]
fn half_graph_scales_samples() {
    let root = install("dodge_burn", "dodge_burn", "half.onnx");
    let package = scan(&root).get("dodge_burn").unwrap().clone();
    let mut image = RgbImage::new(1, 1).unwrap();
    image.set(0, 0, [0.8, 0.4, 0.2]);
    let out = infer(&package, &image).unwrap();
    let px = out.get(0, 0);
    assert!((px[0] - 0.4).abs() < 1e-5, "{px:?}");
    assert!((px[1] - 0.2).abs() < 1e-5, "{px:?}");
    assert!((px[2] - 0.1).abs() < 1e-5, "{px:?}");
}

#[cfg(all(feature = "onnx", not(target_arch = "wasm32")))]
#[test]
fn a_new_package_id_uses_the_same_inference_path() {
    let root = install("glow", "glow", "identity.onnx");
    let package = scan(&root).get("glow").unwrap().clone();
    assert_eq!(package.id, "glow");
    assert_eq!(package.label, "glow");
    let mut image = RgbImage::new(2, 1).unwrap();
    image.set(0, 0, [0.2, 0.4, 0.6]);
    assert_eq!(infer(&package, &image).unwrap(), image);
}

#[cfg(all(feature = "onnx", not(target_arch = "wasm32")))]
#[test]
fn tiled_identity_keeps_the_owned_squares() {
    let root = std::env::temp_dir().join(format!("pc-portrait-onnx-tile-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let pkg = root.join("glow");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("manifest.json"),
        r#"{"formatVersion":1,"id":"glow","label":"Glow","tile":{"size":2,"overlap":1},"tensor":{"input":"image","output":"image_out"}}"#,
    )
    .unwrap();
    std::fs::copy(fixture("identity.onnx"), pkg.join("model.onnx")).unwrap();
    let package = scan(&root).get("glow").unwrap().clone();
    let mut image = RgbImage::new(4, 2).unwrap();
    image.rgb.fill(0.25);
    image.set(3, 1, [1.0, 0.0, 0.5]);
    let out = infer(&package, &image).unwrap();
    assert_eq!(out, image);
}

#[cfg(all(feature = "onnx", not(target_arch = "wasm32")))]
#[test]
fn fixed_spatial_graph_resizes_back_to_the_photo() {
    let root = install("skin_tone", "skin_tone", "fixed4.onnx");
    let package = scan(&root).get("skin_tone").unwrap().clone();
    let mut image = RgbImage::new(2, 2).unwrap();
    image.rgb.fill(0.25);
    let out = infer(&package, &image).unwrap();
    assert_eq!((out.width, out.height), (2, 2));
    assert!(out.rgb.iter().all(|s| (*s - 0.25).abs() < 1e-5), "{:?}", out.rgb);
}

#[cfg(not(all(feature = "onnx", not(target_arch = "wasm32"))))]
#[test]
fn default_build_reports_the_runtime_is_absent() {
    assert!(!runtime_available());
    let root = install("heal", "heal", "identity.onnx");
    let package = scan(&root).get("heal").unwrap().clone();
    let image = RgbImage::new(1, 1).unwrap();
    let err = infer(&package, &image).unwrap_err().to_string();
    assert!(err.contains("ONNX Runtime"), "{err}");
}
