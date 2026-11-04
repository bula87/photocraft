//! Package ids, default blends, and why a menu row is disabled.
//!
//! Ids are strings from the manifest. The classic three are only default blends, Skin Tone's
//! difference-map default, and a stable apply order, not a closed set the host has to recompile
//! to extend.

use photocraft_color::BlendMode;

use crate::scan::Catalog;

/// Soft Light opacity for Dodge & Burn, alone and in the All stack.
pub const DODGE_BURN_OPACITY: f32 = 0.85;

/// Which Filter › Portrait retouch item a caller is asking about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuWhich<'a> {
    Package(&'a str),
    All,
}

/// Blend mode, opacity and layer name for one new layer. Not a destructive paint.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerPlacement {
    pub blend: BlendMode,
    pub opacity: f32,
    pub name: String,
}

/// What the new layer stores after the model returns.
///
/// `Absolute` is the model RGB. `Difference` is the mid-gray residual
/// `clamp(0.5 + 0.5 * (filtered - source), 0, 1)`. Linear Light over `source` reconstructs
/// `filtered` when both images are in 0–1. See `docs/PORTRAIT_FILTERS.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultKind {
    Absolute,
    Difference,
}

impl ResultKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Absolute => "absolute",
            Self::Difference => "difference",
        }
    }

    /// `absolute` / `full`, or `difference` / `diff` / `residual`. Anything else is an error.
    pub fn parse(name: &str) -> Result<Self, crate::Error> {
        let key: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).flat_map(|c| c.to_lowercase()).collect();
        match key.as_str() {
            "absolute" | "full" => Ok(Self::Absolute),
            "difference" | "diff" | "residual" => Ok(Self::Difference),
            _ => Err(crate::Error::Package(format!("unknown result `{name}` (expected absolute or difference)"))),
        }
    }
}

/// One row the Filter menu builds from a scan. `menu_id` is what the UI dispatches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuEntry {
    pub id: String,
    pub label: String,
    pub menu_id: String,
}

/// Defaults for `id` before the manifest overrides them.
pub struct PackageDefaults {
    pub label: String,
    pub priority: i32,
    pub solo: LayerPlacement,
    pub stacked: LayerPlacement,
    pub result: ResultKind,
}

/// `heal`, `dodge_burn` / `dodge-burn`, `skin_tone` / `skin-tone`. Anything else is `None`.
pub fn classic_id(raw: &str) -> Option<&'static str> {
    let key: String = raw.trim().chars().filter(|c| c.is_ascii_alphanumeric()).flat_map(|c| c.to_lowercase()).collect();
    match key.as_str() {
        "heal" => Some("heal"),
        "dodgeburn" | "dodgeandburn" => Some("dodge_burn"),
        "skintone" => Some("skin_tone"),
        _ => None,
    }
}

/// Canonical package id: classic aliases fold, every other id is kept as written.
pub fn normalize_id(raw: &str) -> Result<String, crate::Error> {
    let trimmed = raw.trim();
    if let Some(classic) = classic_id(trimmed) {
        return Ok(classic.to_string());
    }
    if is_safe_id(trimmed) {
        return Ok(trimmed.to_string());
    }
    Err(crate::Error::Package(format!("package id `{trimmed}` must be 1–64 characters of letters, digits, `.`, `_` or `-`")))
}

pub fn is_safe_id(raw: &str) -> bool {
    let mut chars = raw.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    raw.len() <= 64 && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

pub fn menu_command_id(package_id: &str) -> String {
    format!("filter.portraitRetouch.pkg.{package_id}")
}

pub fn defaults_for(id: &str) -> PackageDefaults {
    let (label, priority, blend, opacity, stacked_blend) = match id {
        "heal" => ("Heal", 10, BlendMode::Normal, 1.0, BlendMode::Normal),
        "dodge_burn" => ("Dodge & Burn", 20, BlendMode::SoftLight, DODGE_BURN_OPACITY, BlendMode::SoftLight),
        // Difference map. Linear Light over the source reconstructs the model RGB, alone and in All.
        "skin_tone" => ("Skin Tone", 30, BlendMode::LinearLight, 1.0, BlendMode::LinearLight),
        _ => ("", 100, BlendMode::Normal, 1.0, BlendMode::Normal),
    };
    let result = if id == "skin_tone" { ResultKind::Difference } else { ResultKind::Absolute };
    let label = if label.is_empty() { id.to_string() } else { label.to_string() };
    let solo = LayerPlacement { blend, opacity, name: label.clone() };
    let stacked = LayerPlacement { blend: stacked_blend, opacity, name: label.clone() };
    PackageDefaults { label, priority, solo, stacked, result }
}

/// Resolved placement. `stacked` is true when All is placing more than one layer.
pub fn placement(solo: &LayerPlacement, stacked_spec: &LayerPlacement, stacked: bool) -> LayerPlacement {
    if stacked { stacked_spec.clone() } else { solo.clone() }
}

/// Menu rows for the packages on disk, in the same order **All** applies them.
pub fn menu_entries(catalog: &Catalog) -> Vec<MenuEntry> {
    catalog
        .in_apply_order()
        .into_iter()
        .map(|package| MenuEntry { menu_id: menu_command_id(&package.id), id: package.id.clone(), label: package.label.clone() })
        .collect()
}

/// Why a menu item is disabled, or `None` when it can run.
///
/// A missing package is reported before a missing ONNX Runtime.
pub fn disable_reason(catalog: &Catalog, runtime: bool, which: MenuWhich<'_>) -> Option<String> {
    match which {
        MenuWhich::Package(id) => {
            let canonical = classic_id(id).unwrap_or(id);
            if catalog.get(canonical).is_none() {
                let dir = catalog.directory.display();
                return Some(format!("portrait filter package `{canonical}` is not installed ({dir})"));
            }
        }
        MenuWhich::All => {
            if catalog.packages.is_empty() {
                let dir = catalog.directory.display();
                return Some(format!("no portrait filter packages are installed ({dir})"));
            }
        }
    }
    if !runtime {
        return Some("this build does not include ONNX Runtime; rebuild with --features portrait-onnx".to_string());
    }
    None
}
