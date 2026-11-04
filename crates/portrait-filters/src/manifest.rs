//! `manifest.json` for one package. Unknown keys are ignored so a newer package still loads.

use std::path::Path;

use photocraft_color::BlendMode;
use serde_json::Value;

use crate::Error;
use crate::task::{LayerPlacement, Task};

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

#[derive(Clone, Debug)]
pub struct ParsedManifest {
    pub task: Option<Task>,
    pub name: Option<String>,
    pub version: String,
    pub model_file: String,
    pub solo: Option<LayerPlacement>,
    pub stacked: Option<LayerPlacement>,
    pub tensor: TensorContract,
}

pub fn parse_manifest(bytes: &[u8], folder_name: &str) -> Result<ParsedManifest, Error> {
    if bytes.len() > 256 * 1024 {
        return Err(Error::Package("manifest.json is larger than 256 KiB".into()));
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|e| Error::Package(format!("manifest.json is not JSON ({e})")))?;
    let obj = value.as_object().ok_or_else(|| Error::Package("manifest.json must be an object".into()))?;
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
    let task = string_any(obj, &["task", "id"]).and_then(Task::parse).or_else(|| Task::parse(folder_name));
    let (solo, stacked) = match task {
        Some(task) => placements(obj.get("placement"), task)?,
        None => (None, None),
    };
    let tensor = tensor_contract(obj.get("tensor").or_else(|| obj.get("io")));
    Ok(ParsedManifest {
        task,
        name: string_any(obj, &["name", "layerName", "layer_name"]).map(str::to_string),
        version: string_any(obj, &["version"]).unwrap_or("").to_string(),
        model_file: string_any(obj, &["model", "modelFile", "model_file"]).unwrap_or("model.onnx").to_string(),
        solo,
        stacked,
        tensor,
    })
}

fn placements(value: Option<&Value>, task: Task) -> Result<(Option<LayerPlacement>, Option<LayerPlacement>), Error> {
    let (mut solo, mut stacked) = task.defaults();
    let Some(obj) = value.and_then(Value::as_object) else {
        return Ok((Some(solo), Some(stacked)));
    };
    if let Some(blend) = string_any(obj, &["blend", "blendMode", "blend_mode"]) {
        solo.blend = parse_blend(blend)?;
    }
    if let Some(opacity) = number_any(obj, &["opacity"]) {
        solo.opacity = opacity_of(opacity)?;
    }
    if let Some(name) = string_any(obj, &["name", "layerName", "layer_name"]) {
        solo.name = name.to_string();
        stacked.name = name.to_string();
    }
    stacked.blend = solo.blend;
    stacked.opacity = solo.opacity;
    if let Some(when) = obj.get("whenStacked").or_else(|| obj.get("when_stacked")).and_then(Value::as_object) {
        if let Some(blend) = string_any(when, &["blend", "blendMode", "blend_mode"]) {
            stacked.blend = parse_blend(blend)?;
        }
        if let Some(opacity) = number_any(when, &["opacity"]) {
            stacked.opacity = opacity_of(opacity)?;
        }
    }
    Ok((Some(solo), Some(stacked)))
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
