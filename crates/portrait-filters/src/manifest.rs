//! `manifest.json` for one package. Unknown keys are ignored so a newer package still loads.

use std::path::Path;

use photocraft_color::BlendMode;
use serde_json::Value;

use crate::Error;
use crate::task::{self, LayerPlacement, ResultKind};

/// How the graph wants its RGB tensor. The model's own shape wins when it is unambiguous.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// `[1, 3, H, W]`
    Nchw,
    /// `[1, H, W, 3]`
    Nhwc,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TensorContract {
    pub layout: Layout,
    pub input: Option<String>,
    pub output: Option<String>,
}

impl Default for TensorContract {
    fn default() -> Self {
        Self { layout: Layout::Nchw, input: None, output: None }
    }
}

/// Optional tile. `size` is the owned square; `overlap` is extra context cropped back off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tile {
    pub size: u32,
    pub overlap: u32,
}

#[derive(Clone, Debug)]
pub struct ParsedManifest {
    pub id: String,
    pub label: String,
    pub description: String,
    pub version: String,
    pub priority: i32,
    pub model_file: String,
    pub solo: LayerPlacement,
    pub stacked: LayerPlacement,
    pub result: ResultKind,
    pub tensor: TensorContract,
    pub tile: Option<Tile>,
}

pub fn parse_manifest(bytes: &[u8], folder_name: &str) -> Result<ParsedManifest, Error> {
    if bytes.len() > 256 * 1024 {
        return Err(Error::Package("manifest.json is larger than 256 KiB".into()));
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|e| Error::Package(format!("manifest.json is not JSON ({e})")))?;
    let obj = value.as_object().ok_or_else(|| Error::Package("manifest.json must be an object".into()))?;
    check_format(obj)?;
    let id = match string_any(obj, &["task", "id"]) {
        Some(raw) => task::normalize_id(raw)?,
        None => task::normalize_id(folder_name)?,
    };
    let defaults = task::defaults_for(&id);
    let (solo, stacked) = placements(obj, defaults.solo, defaults.stacked)?;
    let result = result_of(obj, defaults.result)?;
    let label = string_any(obj, &["label", "name"]).unwrap_or(&defaults.label).to_string();
    let mut solo = solo;
    let mut stacked = stacked;
    if let Some(layer) = string_any(obj, &["layerName", "layer_name"]) {
        solo.name = layer.to_string();
        stacked.name = layer.to_string();
    } else if string_any(obj, &["label", "name"]).is_some() {
        solo.name = label.clone();
        stacked.name = label.clone();
    }
    Ok(ParsedManifest {
        id,
        label,
        description: description_of(obj)?,
        version: string_any(obj, &["version"]).unwrap_or("").to_string(),
        priority: priority_of(obj, defaults.priority)?,
        model_file: string_any(obj, &["model", "modelFile", "model_file"]).unwrap_or("model.onnx").to_string(),
        solo,
        stacked,
        result,
        tensor: tensor_contract(obj.get("tensor").or_else(|| obj.get("io"))),
        tile: tile_of(obj.get("tile"))?,
    })
}

fn check_format(obj: &serde_json::Map<String, Value>) -> Result<(), Error> {
    if let Some(format) = string_any(obj, &["format", "schema"])
        && !matches!(format, "photocraft-portrait-filter" | "photocraft-retouch-filter" | "photocraft-retouch")
    {
        return Err(Error::Package(format!("unsupported package format `{format}`")));
    }
    if let Some(version) = obj.get("formatVersion").or_else(|| obj.get("format_version"))
        && version.as_u64() != Some(1)
    {
        return Err(Error::Package(format!("unsupported formatVersion {version}")));
    }
    Ok(())
}

fn placements(obj: &serde_json::Map<String, Value>, mut solo: LayerPlacement, mut stacked: LayerPlacement) -> Result<(LayerPlacement, LayerPlacement), Error> {
    // A blend written on the package applies to both rows. Skin Tone's Linear Light default
    // survives when the manifest leaves blend unset. `whenStacked` only changes the All row.
    if has_blend_or_opacity(obj) {
        apply_blend_opacity(obj, &mut solo)?;
        apply_blend_opacity(obj, &mut stacked)?;
    }
    if let Some(when) = stacked_block(obj) {
        apply_blend_opacity(when, &mut stacked)?;
    }
    if let Some(placement) = obj.get("placement").and_then(Value::as_object) {
        if has_blend_or_opacity(placement) {
            apply_blend_opacity(placement, &mut solo)?;
            if stacked_block(placement).is_none() && stacked_block(obj).is_none() {
                stacked.blend = solo.blend;
                stacked.opacity = solo.opacity;
            }
        }
        if let Some(name) = string_any(placement, &["name", "layerName", "layer_name"]) {
            solo.name = name.to_string();
            stacked.name = name.to_string();
        }
        if let Some(when) = stacked_block(placement) {
            apply_blend_opacity(when, &mut stacked)?;
        }
    }
    Ok((solo, stacked))
}

fn result_of(obj: &serde_json::Map<String, Value>, default: ResultKind) -> Result<ResultKind, Error> {
    let Some(key) = ["result", "resultMode", "result_mode"].into_iter().find(|key| obj.contains_key(*key)) else {
        return Ok(default);
    };
    let Some(raw) = obj.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()) else {
        return Err(Error::Package("result must be \"absolute\" or \"difference\"".into()));
    };
    ResultKind::parse(raw)
}

fn has_blend_or_opacity(obj: &serde_json::Map<String, Value>) -> bool {
    string_any(obj, &["blend", "blendMode", "blend_mode"]).is_some() || number_any(obj, &["opacity"]).is_some()
}

fn stacked_block(obj: &serde_json::Map<String, Value>) -> Option<&serde_json::Map<String, Value>> {
    obj.get("whenStacked").or_else(|| obj.get("when_stacked")).and_then(Value::as_object)
}

fn apply_blend_opacity(obj: &serde_json::Map<String, Value>, place: &mut LayerPlacement) -> Result<(), Error> {
    if let Some(blend) = string_any(obj, &["blend", "blendMode", "blend_mode"]) {
        place.blend = parse_blend(blend)?;
    }
    if let Some(opacity) = number_any(obj, &["opacity"]) {
        place.opacity = opacity_of(opacity)?;
    }
    Ok(())
}

fn tensor_contract(value: Option<&Value>) -> TensorContract {
    let Some(obj) = value.and_then(Value::as_object) else {
        return TensorContract::default();
    };
    let layout = match string_any(obj, &["layout"]).unwrap_or("nchw").to_ascii_lowercase().as_str() {
        "nhwc" => Layout::Nhwc,
        _ => Layout::Nchw,
    };
    TensorContract {
        layout,
        input: string_any(obj, &["input", "inputName", "input_name"]).map(str::to_string),
        output: string_any(obj, &["output", "outputName", "output_name"]).map(str::to_string),
    }
}

fn tile_of(value: Option<&Value>) -> Result<Option<Tile>, Error> {
    let Some(obj) = value.and_then(Value::as_object) else { return Ok(None) };
    let Some(size) = number_any(obj, &["size"]) else {
        return Err(Error::Package("tile.size is required when tile is set".into()));
    };
    if !size.is_finite() || !(1.0..=4096.0).contains(&size) || size.fract() != 0.0 {
        return Err(Error::Package("tile.size must be a whole number from 1 to 4096".into()));
    }
    let overlap = number_any(obj, &["overlap"]).unwrap_or(0.0);
    if !overlap.is_finite() || overlap < 0.0 || overlap.fract() != 0.0 || overlap >= size {
        return Err(Error::Package("tile.overlap must be a whole number smaller than tile.size".into()));
    }
    Ok(Some(Tile { size: size as u32, overlap: overlap as u32 }))
}

fn priority_of(obj: &serde_json::Map<String, Value>, default: i32) -> Result<i32, Error> {
    let Some(value) = obj.get("priority").or_else(|| obj.get("order")) else { return Ok(default) };
    let Some(number) = value.as_f64() else { return Err(Error::Package("priority must be a whole number".into())) };
    if !number.is_finite() || number.fract() != 0.0 || !(-10_000.0..=10_000.0).contains(&number) {
        return Err(Error::Package("priority must be a whole number from -10000 to 10000".into()));
    }
    Ok(number as i32)
}

fn description_of(obj: &serde_json::Map<String, Value>) -> Result<String, Error> {
    let Some(text) = string_any(obj, &["description"]) else { return Ok(String::new()) };
    if text.len() > 1024 {
        return Err(Error::Package("description is longer than 1024 bytes".into()));
    }
    Ok(text.to_string())
}

pub fn parse_blend(name: &str) -> Result<BlendMode, Error> {
    let norm = |raw: &str| raw.to_ascii_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>();
    let want = norm(name);
    std::iter::once(BlendMode::PassThrough)
        .chain(BlendMode::layer_modes())
        .filter(|mode| *mode != BlendMode::PassThrough)
        .find(|mode| norm(mode.label()) == want || norm(&format!("{mode:?}")) == want)
        .ok_or_else(|| Error::Package(format!("unknown blend mode `{name}`")))
}

fn opacity_of(value: f64) -> Result<f32, Error> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(Error::Package("opacity must be a number from 0 to 1".into()));
    }
    Ok(value as f32)
}

fn string_any<'a>(obj: &'a serde_json::Map<String, Value>, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| obj.get(*key).and_then(Value::as_str)).filter(|s| !s.is_empty())
}

fn number_any(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| obj.get(*key).and_then(Value::as_f64))
}

/// Reject a model filename that would leave the package directory.
pub fn model_file_ok(name: &str) -> bool {
    let path = Path::new(name);
    !name.is_empty() && path.components().count() == 1 && path.file_name().is_some_and(|n| n == name)
}
