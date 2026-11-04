//! Package scan, menu availability and layer placement. No trained weights.

use std::fs;
use std::path::{Path, PathBuf};

use photocraft_color::BlendMode;
use photocraft_portrait_filters::{Catalog, DODGE_BURN_OPACITY, MenuWhich, Platform, ResultKind, default_directory_for, disable_reason, menu_entries, scan};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pc-portrait-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn package(root: &Path, folder: &str, manifest: &str) {
    let dir = root.join(folder);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("manifest.json"), manifest).unwrap();
    fs::write(dir.join("model.onnx"), b"not-a-real-graph").unwrap();
}

#[test]
fn default_directories_match_the_platform_rule() {
    let home = Path::new("/home/retoucher");
    let mac = default_directory_for(Platform::Macos, home, None);
    assert_eq!(mac, Path::new("/home/retoucher/Library/Application Support/PhotoCraft/Portrait Filters"));
    let xdg = default_directory_for(Platform::Other, home, Some(Path::new("/var/data")));
    assert_eq!(xdg, Path::new("/var/data/photocraft/portrait-filters"));
    let fallback = default_directory_for(Platform::Other, home, None);
    assert_eq!(fallback, Path::new("/home/retoucher/.local/share/photocraft/portrait-filters"));
}

#[test]
fn scan_reads_task_folders_and_skips_broken_ones() {
    let root = scratch("scan");
    package(&root, "heal", r#"{"format":"photocraft-portrait-filter","formatVersion":1,"task":"heal"}"#);
    package(&root, "dodge-burn", r#"{"task":"dodge_burn","name":"Dodge & Burn"}"#);
    package(&root, "skin_tone", "{}");
    let notes = root.join("notes");
    fs::create_dir_all(&notes).unwrap();
    fs::write(notes.join("manifest.json"), b"{").unwrap();
    fs::write(notes.join("model.onnx"), b"x").unwrap();
    fs::create_dir_all(root.join("empty")).unwrap();
    fs::write(root.join("README.txt"), b"not a package").unwrap();
    let catalog = scan(&root);
    assert!(catalog.note.is_none(), "{:?}", catalog.note);
    assert_eq!(catalog.packages.len(), 3);
    assert_eq!(catalog.get("heal").unwrap().solo.blend, BlendMode::Normal);
    assert_eq!(catalog.get("heal").unwrap().result, ResultKind::Absolute);
    assert!((catalog.get("heal").unwrap().solo.opacity - 1.0).abs() < 1e-6);
    let dodge = catalog.get("dodge_burn").unwrap();
    assert_eq!(dodge.solo.blend, BlendMode::SoftLight);
    assert_eq!(dodge.solo.opacity.to_bits(), DODGE_BURN_OPACITY.to_bits());
    assert_eq!(dodge.stacked.blend, BlendMode::SoftLight);
    assert_eq!(dodge.result, ResultKind::Absolute, "Dodge & Burn keeps the model's Soft Light map");
    let skin = catalog.get("skin_tone").unwrap();
    assert_eq!(skin.solo.blend, BlendMode::LinearLight, "alone, Skin Tone is a difference map");
    assert_eq!(skin.stacked.blend, BlendMode::LinearLight, "in the All stack, Skin Tone stays Linear Light");
    assert!((skin.solo.opacity - 1.0).abs() < 1e-6);
    assert!((skin.stacked.opacity - 1.0).abs() < 1e-6);
    assert_eq!(skin.result, ResultKind::Difference, "an old manifest that omits result still encodes a difference");
    assert_eq!(skin.solo.name, "Skin Tone");
    let skipped: Vec<_> = catalog.skipped.iter().map(|s| s.folder.as_str()).collect();
    assert!(skipped.contains(&"notes"), "{skipped:?}");
    assert!(skipped.contains(&"empty"), "{skipped:?}");
    assert!(!skipped.contains(&"README.txt"));
}

#[test]
fn a_new_task_id_joins_the_catalog_without_a_code_change() {
    let root = scratch("glow");
    package(&root, "heal", r#"{"task":"heal"}"#);
    package(&root, "dodge_burn", r#"{"task":"dodge_burn"}"#);
    package(&root, "skin_tone", r#"{"task":"skin_tone"}"#);
    package(
        &root,
        "glow",
        r#"{"format":"photocraft-portrait-filter","formatVersion":1,"id":"glow","label":"Glow","description":"Soft halo","blend":"screen","opacity":0.6,"layerName":"Glow layer","priority":15,"tile":{"size":64,"overlap":8},"unused":true}"#,
    );
    let catalog = scan(&root);
    let glow = catalog.get("glow").unwrap();
    assert_eq!(glow.label, "Glow");
    assert_eq!(glow.description, "Soft halo");
    assert_eq!(glow.solo.name, "Glow layer");
    assert_eq!(glow.solo.blend, BlendMode::Screen);
    assert!((glow.solo.opacity - 0.6).abs() < 1e-6);
    assert_eq!(glow.priority, 15);
    assert_eq!(glow.tile.unwrap().size, 64);
    let order: Vec<_> = catalog.in_apply_order().into_iter().map(|p| p.id.as_str()).collect();
    assert_eq!(order, ["heal", "glow", "dodge_burn", "skin_tone"]);
    let menu: Vec<_> = menu_entries(&catalog).into_iter().map(|e| (e.menu_id, e.label)).collect();
    assert_eq!(
        menu,
        vec![
            ("filter.portraitRetouch.pkg.heal".into(), "Heal".into()),
            ("filter.portraitRetouch.pkg.glow".into(), "Glow".into()),
            ("filter.portraitRetouch.pkg.dodge_burn".into(), "Dodge & Burn".into()),
            ("filter.portraitRetouch.pkg.skin_tone".into(), "Skin Tone".into()),
        ]
    );
    assert!(disable_reason(&catalog, true, MenuWhich::Package("glow")).is_none());
    assert!(disable_reason(&catalog, true, MenuWhich::All).is_none());
}

#[test]
fn missing_directory_is_an_empty_catalog() {
    let missing = scratch("missing").join("absent");
    let catalog = scan(&missing);
    assert!(catalog.packages.is_empty());
    assert!(catalog.note.unwrap().contains("does not exist"));
}

#[test]
fn menu_disables_missing_packages_and_a_build_without_the_runtime() {
    let root = scratch("menu");
    package(&root, "heal", r#"{"task":"heal"}"#);
    let catalog = scan(&root);
    let heal = disable_reason(&catalog, false, MenuWhich::Package("heal"));
    assert!(heal.unwrap().contains("ONNX Runtime"), "a present package still needs the runtime");
    let dodge = disable_reason(&catalog, true, MenuWhich::Package("dodge_burn")).unwrap();
    assert!(dodge.contains("dodge_burn"), "{dodge}");
    assert!(dodge.contains(root.to_str().unwrap()), "{dodge}");
    assert!(disable_reason(&catalog, true, MenuWhich::All).is_none(), "All runs whatever is installed");
    assert!(disable_reason(&catalog, true, MenuWhich::Package("heal")).is_none());
}

#[test]
fn all_runs_every_discovered_package_when_the_runtime_is_present() {
    let root = scratch("all");
    package(&root, "skin_tone", r#"{"task":"skin_tone"}"#);
    let one = scan(&root);
    assert!(disable_reason(&one, true, MenuWhich::All).is_none());
    assert_eq!(one.get("skin_tone").unwrap().stacked.blend, BlendMode::LinearLight);
    assert_eq!(one.get("skin_tone").unwrap().result, ResultKind::Difference);
    for task in ["heal", "dodge_burn"] {
        package(&root, task, &format!(r#"{{"task":"{task}"}}"#));
    }
    let catalog = scan(&root);
    assert!(disable_reason(&catalog, true, MenuWhich::All).is_none());
    assert!(disable_reason(&catalog, false, MenuWhich::All).unwrap().contains("ONNX Runtime"));
    let order: Vec<_> = catalog.in_apply_order().into_iter().map(|p| p.id.as_str()).collect();
    assert_eq!(order, ["heal", "dodge_burn", "skin_tone"]);
}

#[test]
fn a_manifest_can_override_blend_and_rejects_a_path_escape() {
    let root = scratch("override");
    package(&root, "heal", r#"{"task":"heal","placement":{"blend":"soft light","opacity":0.5,"whenStacked":{"blend":"multiply","opacity":0.4}}}"#);
    let heal = scan(&root).get("heal").unwrap().clone();
    assert_eq!(heal.solo.blend, BlendMode::SoftLight);
    assert!((heal.solo.opacity - 0.5).abs() < 1e-6);
    assert_eq!(heal.stacked.blend, BlendMode::Multiply);
    let bad = root.join("dodge_burn");
    fs::create_dir_all(&bad).unwrap();
    fs::write(bad.join("manifest.json"), r#"{"task":"dodge_burn","model":"../model.onnx"}"#).unwrap();
    fs::write(bad.join("model.onnx"), b"x").unwrap();
    let skipped = scan(&root).skipped;
    assert!(skipped.iter().any(|s| s.reason.contains("file name")), "{skipped:?}");
}

#[test]
fn result_difference_is_opt_in_and_skin_tone_can_store_model_rgb() {
    let root = scratch("result");
    package(&root, "glow", r#"{"id":"glow","label":"Glow","result":"difference"}"#);
    package(&root, "matte", r#"{"id":"matte","result":"diff"}"#);
    package(&root, "heal", r#"{"task":"heal","resultMode":"full"}"#);
    package(&root, "skin_tone", r#"{"task":"skin_tone","result":"absolute","blend":"color"}"#);
    package(&root, "dodge_burn", r#"{"task":"dodge-burn","result_mode":"residual"}"#);
    let catalog = scan(&root);
    assert_eq!(catalog.get("glow").unwrap().result, ResultKind::Difference);
    assert_eq!(catalog.get("glow").unwrap().solo.blend, BlendMode::Normal, "difference does not change another id's blend");
    assert_eq!(catalog.get("matte").unwrap().result, ResultKind::Difference);
    assert_eq!(catalog.get("heal").unwrap().result, ResultKind::Absolute);
    assert_eq!(catalog.get("heal").unwrap().solo.blend, BlendMode::Normal);
    let skin = catalog.get("skin_tone").unwrap();
    assert_eq!(skin.result, ResultKind::Absolute);
    assert_eq!(skin.solo.blend, BlendMode::Color, "an explicit blend still wins");
    assert_eq!(skin.stacked.blend, BlendMode::Color);
    let dodge = catalog.get("dodge_burn").unwrap();
    assert_eq!(dodge.result, ResultKind::Difference);
    assert_eq!(dodge.solo.blend, BlendMode::SoftLight);
    for (folder, manifest) in [("nope", r#"{"id":"nope","result":"softlight"}"#), ("typed", r#"{"id":"typed","result":1}"#)] {
        let dir = root.join(folder);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("manifest.json"), manifest).unwrap();
        fs::write(dir.join("model.onnx"), b"x").unwrap();
    }
    let skipped = scan(&root).skipped;
    assert!(skipped.iter().any(|s| s.folder == "nope" && s.reason.contains("unknown result")), "{skipped:?}");
    assert!(skipped.iter().any(|s| s.folder == "typed" && s.reason.contains("result")), "{skipped:?}");
}

#[test]
fn duplicate_tasks_keep_the_first_folder_in_sorted_order() {
    let root = scratch("dup");
    package(&root, "a-heal", r#"{"task":"heal","name":"First"}"#);
    package(&root, "b-heal", r#"{"task":"heal","name":"Second"}"#);
    let catalog = scan(&root);
    assert_eq!(catalog.get("heal").unwrap().solo.name, "First");
    assert!(catalog.skipped.iter().any(|s| s.folder == "b-heal"));
}

#[test]
fn empty_catalog_reports_every_task_for_all() {
    let catalog = Catalog::unavailable(PathBuf::from("/nowhere"), "missing");
    let reason = disable_reason(&catalog, true, MenuWhich::All).unwrap();
    assert!(reason.contains("no portrait filter packages"), "{reason}");
}
