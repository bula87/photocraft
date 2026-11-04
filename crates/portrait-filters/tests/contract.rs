//! Package scan, menu availability and layer placement. No trained weights.

use std::fs;
use std::path::{Path, PathBuf};

use photocraft_color::BlendMode;
use photocraft_portrait_filters::{Catalog, DODGE_BURN_OPACITY, MenuWhich, Platform, Task, default_directory_for, disable_reason, scan};

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
    package(&root, "notes", r#"{"task":"frequency"}"#);
    fs::create_dir_all(root.join("empty")).unwrap();
    fs::write(root.join("README.txt"), b"not a package").unwrap();
    let catalog = scan(&root);
    assert!(catalog.note.is_none(), "{:?}", catalog.note);
    assert_eq!(catalog.packages.len(), 3);
    assert_eq!(catalog.get(Task::Heal).unwrap().solo.blend, BlendMode::Normal);
    assert!((catalog.get(Task::Heal).unwrap().solo.opacity - 1.0).abs() < 1e-6);
    let dodge = catalog.get(Task::DodgeBurn).unwrap();
    assert_eq!(dodge.solo.blend, BlendMode::SoftLight);
    assert_eq!(dodge.solo.opacity.to_bits(), DODGE_BURN_OPACITY.to_bits());
    assert_eq!(dodge.stacked.blend, BlendMode::SoftLight);
    let skin = catalog.get(Task::SkinTone).unwrap();
    assert_eq!(skin.solo.blend, BlendMode::Normal, "alone, Skin Tone keeps the model RGB");
    assert_eq!(skin.stacked.blend, BlendMode::Color, "in the All stack, Skin Tone is Color");
    assert_eq!(skin.solo.name, "Skin Tone");
    let skipped: Vec<_> = catalog.skipped.iter().map(|s| s.folder.as_str()).collect();
    assert!(skipped.contains(&"notes"), "{skipped:?}");
    assert!(skipped.contains(&"empty"), "{skipped:?}");
    assert!(!skipped.contains(&"README.txt"));
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
    let heal = disable_reason(&catalog, false, MenuWhich::Task(Task::Heal));
    assert!(heal.unwrap().contains("ONNX Runtime"), "a present package still needs the runtime");
    let dodge = disable_reason(&catalog, true, MenuWhich::Task(Task::DodgeBurn)).unwrap();
    assert!(dodge.contains("dodge_burn"), "{dodge}");
    assert!(dodge.contains(root.to_str().unwrap()), "{dodge}");
    let all = disable_reason(&catalog, true, MenuWhich::All).unwrap();
    assert!(all.contains("dodge_burn") && all.contains("skin_tone"), "{all}");
    assert!(disable_reason(&catalog, true, MenuWhich::Task(Task::Heal)).is_none());
}

#[test]
fn all_is_available_only_when_every_package_and_the_runtime_are_present() {
    let root = scratch("all");
    for task in ["heal", "dodge_burn", "skin_tone"] {
        package(&root, task, &format!(r#"{{"task":"{task}"}}"#));
    }
    let catalog = scan(&root);
    assert!(disable_reason(&catalog, true, MenuWhich::All).is_none());
    assert!(disable_reason(&catalog, false, MenuWhich::All).unwrap().contains("ONNX Runtime"));
}

#[test]
fn a_manifest_can_override_blend_and_rejects_a_path_escape() {
    let root = scratch("override");
    package(&root, "heal", r#"{"task":"heal","placement":{"blend":"soft light","opacity":0.5,"whenStacked":{"blend":"multiply","opacity":0.4}}}"#);
    let heal = scan(&root).get(Task::Heal).unwrap().clone();
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
fn duplicate_tasks_keep_the_first_folder_in_sorted_order() {
    let root = scratch("dup");
    package(&root, "a-heal", r#"{"task":"heal","name":"First"}"#);
    package(&root, "b-heal", r#"{"task":"heal","name":"Second"}"#);
    let catalog = scan(&root);
    assert_eq!(catalog.get(Task::Heal).unwrap().solo.name, "First");
    assert!(catalog.skipped.iter().any(|s| s.folder == "b-heal"));
}

#[test]
fn empty_catalog_reports_every_task_for_all() {
    let catalog = Catalog::unavailable(PathBuf::from("/nowhere"), "missing");
    let reason = disable_reason(&catalog, true, MenuWhich::All).unwrap();
    assert!(reason.contains("heal, dodge_burn, skin_tone"), "{reason}");
}
