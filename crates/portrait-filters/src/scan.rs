//! Scan a Portrait Filters directory: one child folder per package.

use std::fs;
use std::path::{Path, PathBuf};

use crate::Error;
use crate::manifest::{self, ParsedManifest, TensorContract, Tile};
use crate::task::{LayerPlacement, ResultKind};

/// One installed package. `solo` is a single menu run; `stacked` is used when All places more than one layer.
#[derive(Clone, Debug)]
pub struct Package {
    pub id: String,
    pub label: String,
    pub description: String,
    pub version: String,
    pub priority: i32,
    pub folder: PathBuf,
    pub model: PathBuf,
    pub solo: LayerPlacement,
    pub stacked: LayerPlacement,
    /// `Absolute` stores the model RGB. `Difference` stores the mid-gray residual.
    pub result: ResultKind,
    pub tensor: TensorContract,
    pub tile: Option<Tile>,
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

    pub fn get(&self, id: &str) -> Option<&Package> {
        let canonical = crate::task::classic_id(id).unwrap_or(id);
        self.packages.iter().find(|package| package.id == canonical || package.id == id)
    }

    /// Bottom-to-top order for **All**: ascending `priority`, then id.
    pub fn in_apply_order(&self) -> Vec<&Package> {
        let mut packages: Vec<&Package> = self.packages.iter().collect();
        packages.sort_by(|a, b| a.priority.cmp(&b.priority).then_with(|| a.id.cmp(&b.id)));
        packages
    }
}

/// Read `directory`'s child folders. Duplicate ids keep the first folder in alphabetical order.
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
                if catalog.get(&package.id).is_some() {
                    catalog.skipped.push(Skip { folder: name, reason: format!("duplicate task `{}`", package.id) });
                } else {
                    catalog.packages.push(package);
                }
            }
            Err(e) => catalog.skipped.push(Skip { folder: name, reason: e.to_string() }),
        }
    }
    catalog.packages.sort_by(|a, b| a.priority.cmp(&b.priority).then_with(|| a.id.cmp(&b.id)));
    catalog
}

fn load_folder(folder: &Path, name: &str) -> Result<Package, Error> {
    let manifest_path = folder.join("manifest.json");
    let bytes = fs::read(&manifest_path).map_err(|_| Error::Package("missing manifest.json".into()))?;
    let parsed = manifest::parse_manifest(&bytes, name)?;
    let ParsedManifest { id, label, description, version, priority, model_file, solo, stacked, result, tensor, tile } = parsed;
    if !manifest::model_file_ok(&model_file) {
        return Err(Error::Package(format!("model file `{model_file}` must be a file name inside the package")));
    }
    let model = folder.join(&model_file);
    let meta = fs::metadata(&model).map_err(|_| Error::Package(format!("missing {model_file}")))?;
    if !meta.is_file() || meta.len() == 0 {
        return Err(Error::Package(format!("{model_file} is empty")));
    }
    Ok(Package { id, label, description, version, priority, folder: folder.to_path_buf(), model, solo, stacked, result, tensor, tile })
}
