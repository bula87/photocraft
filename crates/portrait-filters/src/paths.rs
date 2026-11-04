//! Where packages live, unless Preferences names another folder.

use std::path::{Path, PathBuf};

/// The platform half of the default directory rule. Tests pass this in; the app uses [`Platform::host`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    /// `~/Library/Application Support/PhotoCraft/Portrait Filters`
    Macos,
    /// `$XDG_DATA_HOME/photocraft/portrait-filters`, or `~/.local/share/photocraft/portrait-filters`.
    Other,
}

impl Platform {
    pub const fn host() -> Self {
        if cfg!(target_os = "macos") { Self::Macos } else { Self::Other }
    }
}

/// Default package directory for this machine, or `None` when there is no home directory (wasm).
pub fn default_directory() -> Option<PathBuf> {
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let home = home_dir()?;
        let xdg = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|p| !p.as_os_str().is_empty());
        Some(default_directory_for(Platform::host(), &home, xdg.as_deref()))
    }
}

/// `override_folder` when it is non-empty, otherwise [`default_directory`].
pub fn resolve_directory(override_folder: &str) -> Option<PathBuf> {
    let trimmed = override_folder.trim();
    if !trimmed.is_empty() {
        return Some(PathBuf::from(trimmed));
    }
    default_directory()
}

/// The default directory for `platform`, given a home directory and an optional `XDG_DATA_HOME`.
pub fn default_directory_for(platform: Platform, home: &Path, xdg_data_home: Option<&Path>) -> PathBuf {
    match platform {
        Platform::Macos => home.join("Library/Application Support/PhotoCraft/Portrait Filters"),
        Platform::Other => match xdg_data_home {
            Some(xdg) if !xdg.as_os_str().is_empty() => xdg.join("photocraft/portrait-filters"),
            _ => home.join(".local/share/photocraft/portrait-filters"),
        },
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn home_dir() -> Option<PathBuf> {
    let raw = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let path = PathBuf::from(raw);
    if path.as_os_str().is_empty() { None } else { Some(path) }
}
