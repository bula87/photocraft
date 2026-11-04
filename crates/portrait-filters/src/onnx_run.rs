//! ONNX Runtime session for one package. Compiled only with `--features onnx` on a native target.

use std::sync::{Mutex, OnceLock};

use ort::session::Session;
use ort::value::Tensor;

use crate::Error;
use crate::image::{self, RgbImage};
use crate::manifest::Layout;
use crate::scan::Package;

pub fn infer(package: &Package, image: &RgbImage) -> Result<RgbImage, Error> {
    let _guard = lock().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let env = environment()?;
    let mut session = Session::builder(env).map_err(model)?.commit_from_file(&package.model).map_err(model)?;
    let input_info = session.inputs().first().ok_or_else(|| Error::Model("model has no inputs".into()))?;
    let input_name = package.tensor.input.clone().unwrap_or_else(|| input_info.name().to_string());
    let shape = tensor_shape(input_info.dtype());
    let layout = resolved_layout(shape.as_deref(), package.tensor.layout)?;
    let (model_w, model_h) = spatial_size(shape.as_deref(), layout)?;
    let fed = match (model_w, model_h) {
        (Some(w), Some(h)) => image::resize(image, w, h)?,
        (None, None) => image.clone(),
        _ => return Err(Error::Model("model must fix both height and width, or neither".into())),
    };
    let (tensor_shape, data) = pack(&fed, layout)?;
    let tensor = Tensor::from_array((tensor_shape, data)).map_err(model)?;
    let fallback_output = session.outputs().first().map(|outlet| outlet.name().to_string());
    let outputs = session.run(ort::inputs![input_name.as_str() => tensor]).map_err(model)?;
    let output_name = package.tensor.output.clone().or(fallback_output).ok_or_else(|| Error::Model("model has no outputs".into()))?;
    let value = outputs.get(output_name.as_str()).ok_or_else(|| Error::Model(format!("model has no output `{output_name}`")))?;
    let (out_shape, out_data) = value.try_extract_tensor::<f32>().map_err(model)?;
    let decoded = decode(&fed, layout, out_shape, out_data)?;
    image::resize(&decoded, image.width, image.height)
}

fn lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn environment() -> Result<&'static ort::environment::Environment, Error> {
    // `OnceLock::get_or_try_init` is still unstable. `infer` holds `lock()`, so only one
    // thread builds the process-wide ONNX Runtime environment.
    static ENV: OnceLock<ort::environment::Environment> = OnceLock::new();
    if let Some(env) = ENV.get() {
        return Ok(env);
    }
    let built = ort::init().with_name("photocraft-portrait-filters").with_telemetry(false).build().map_err(model)?;
    let _ = ENV.set(built);
    ENV.get().ok_or_else(|| Error::Model("ONNX Runtime environment was not stored".into()))
}

fn model(err: impl std::fmt::Display) -> Error {
    Error::Model(err.to_string())
}

fn tensor_shape(dtype: &ort::value::ValueType) -> Option<Vec<i64>> {
    match dtype {
        ort::value::ValueType::Tensor { shape, .. } => Some(shape.iter().copied().collect()),
        _ => None,
    }
}

fn resolved_layout(shape: Option<&[i64]>, declared: Layout) -> Result<Layout, Error> {
    let Some(shape) = shape else { return Ok(declared) };
    if shape.len() != 4 {
        return Err(Error::Model(format!("model input rank is {}, expected 4", shape.len())));
    }
    match (shape.get(1).copied(), shape.get(3).copied()) {
        (Some(3), Some(c)) if c != 3 => Ok(Layout::Nchw),
        (Some(c), Some(3)) if c != 3 => Ok(Layout::Nhwc),
        _ => Ok(declared),
    }
}

fn spatial_size(shape: Option<&[i64]>, layout: Layout) -> Result<(Option<u32>, Option<u32>), Error> {
    let Some(shape) = shape else { return Ok((None, None)) };
    let (height, width) = match layout {
        Layout::Nchw => (shape.get(2).copied(), shape.get(3).copied()),
        Layout::Nhwc => (shape.get(1).copied(), shape.get(2).copied()),
    };
    Ok((dim(width)?, dim(height)?))
}

fn dim(value: Option<i64>) -> Result<Option<u32>, Error> {
    match value {
        Some(v) if v > 0 => u32::try_from(v).map(Some).map_err(|_| Error::TooLarge),
        _ => Ok(None),
    }
}

fn pack(image: &RgbImage, layout: Layout) -> Result<(Vec<i64>, Vec<f32>), Error> {
    let h = i64::from(image.height);
    let w = i64::from(image.width);
    match layout {
        Layout::Nchw => Ok((vec![1, 3, h, w], image::to_nchw(image))),
        Layout::Nhwc => Ok((vec![1, h, w, 3], image.rgb.clone())),
    }
}

fn decode(fed: &RgbImage, layout: Layout, shape: &ort::value::Shape, data: &[f32]) -> Result<RgbImage, Error> {
    let dims: Vec<i64> = shape.iter().copied().collect();
    if dims.len() != 4 {
        return Err(Error::Model(format!("model output rank is {}, expected 4", dims.len())));
    }
    let (h, w) = match layout {
        Layout::Nchw => (dims.get(2).copied().unwrap_or(-1), dims.get(3).copied().unwrap_or(-1)),
        Layout::Nhwc => (dims.get(1).copied().unwrap_or(-1), dims.get(2).copied().unwrap_or(-1)),
    };
    let (height, width) = match (positive(h)?, positive(w)?) {
        (Some(height), Some(width)) => (height, width),
        _ => (fed.height, fed.width),
    };
    match layout {
        Layout::Nchw => image::from_nchw(width, height, data),
        Layout::Nhwc => image::from_nhwc(width, height, data),
    }
}

fn positive(value: i64) -> Result<Option<u32>, Error> {
    if value <= 0 { Ok(None) } else { u32::try_from(value).map(Some).map_err(|_| Error::TooLarge) }
}
