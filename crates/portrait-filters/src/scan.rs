//! Scan a Portrait Filters directory: one child folder per package.

use std::fs;
use std::path::{Path, PathBuf};

use crate::Error;
use crate::manifest::{self, ParsedManifest, TensorContract};
use crate::task::{LayerPlacement, STACK_ORDER, Task};

/// One installed package. `solo` is Heal / Dodge & Burn / Skin Tone alone; `stacked` is the All stack.
#[derive(Clone, Debug)]
pub struct Package {
    pub task: Task,
    pub version: String,
    pub folder: PathBuf,
    pub model: PathBuf,
    pub solo: LayerPlacement,
    pub stacked: LayerPlacement,
    pub tensor: TensorContract,
}

/// A child folder that was not a usable package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skip {
    pub folder: String,
    pub reason: String,
}

/// The result of one scan. A missing directory is an empty catalog, not an error.
#[derive(Clone, Debug)]
pub struct Catalog {
    pub directory: PathBuf,
    pub packages: Vec<Package>,
    pub skipped: Vec<Skip>,
    pub note: Option<String>,
}

impl Catalog {
    pub fn unavailable(directory: PathBuf, note: impl Into<String>) -> Self {
        Self { directory, packages: Vec::new(), skipped: Vec::new(), note: Some(note.into()) }
    }

    pub fn get(&self, task: Task) -> Option<&Package> {
        self.packages.iter().find(|package| package.task == task)
    }
}

/// Read `directory`'s child folders. Order is [`STACK_ORDER`], then any duplicate is skipped.
pub fn scan(directory: &Path) -> Catalog {
    let mut catalog = Catalog { directory: directory.to_path_buf(), packages: Vec::new(), skipped: Vec::new(), note: None };
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            catalog.note = Some("directory does not exist yet".into());
            return catalog;
        }
        Err(e) => {
            catalog.note = Some(format!("cannot read directory ({e})"));
            return catalog;
        }
    };
    let mut names = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => names.push(name),
            Ok(_) => {}
            Err(e) => catalog.skipped.push(Skip { folder: name, reason: e.to_string() }),
        }
    }
    names.sort();
    for name in names {
        match load_folder(&directory.join(&name), &name) {
            Ok(package) => {
                if catalog.get(package.task).is_some() {
                    catalog.skipped.push(Skip { folder: name, reason: format!("duplicate task `{}`", package.task.id()) });
                } else {
                    catalog.packages.push(package);
                }
            }
            Err(e) => catalog.skipped.push(Skip { folder: name, reason: e.to_string() }),
        }
    }
    catalog.packages.sort_by_key(|package| STACK_ORDER.iter().position(|task| *task == package.task).unwrap_or(usize::MAX));
    catalog
}

fn load_folder(folder: &Path, name: &str) -> Result<Package, Error> {
    let manifest_path = folder.join("manifest.json");
    let bytes = fs::read(&manifest_path).map_err(|_| Error::Package("missing manifest.json".into()))?;
    let parsed = manifest::parse_manifest(&bytes, name)?;
    let ParsedManifest { task, name: layer_name, version, model_file, solo, stacked, tensor } = parsed;
    let task = task.ok_or_else(|| Error::Package(format!("folder `{name}` is not heal, dodge_burn or skin_tone")))?;
    if !manifest::model_file_ok(&model_file) {
        return Err(Error::Package(format!("model file `{model_file}` must be a file name inside the package")));
    }
    let model = folder.join(&model_file);
    let meta = fs::metadata(&model).map_err(|_| Error::Package(format!("missing {model_file}")))?;
    if !meta.is_file() || meta.len() == 0 {
        return Err(Error::Package(format!("{model_file} is empty")));
    }
    let (default_solo, default_stacked) = task.defaults();
    let mut solo = solo.unwrap_or(default_solo);
    let mut stacked = stacked.unwrap_or(default_stacked);
    if let Some(layer_name) = layer_name {
        solo.name = layer_name.clone();
        if stacked.name == task.menu_label() {
            stacked.name = layer_name;
        }
    }
    Ok(Package { task, version, folder: folder.to_path_buf(), model, solo, stacked, tensor })
}
