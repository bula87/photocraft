//! RGB images handed to a package. Samples are finite and in 0–1 after a model returns.
//!
//! Packing and resize are used by the ONNX runner. Without that feature they stay in the crate
//! so the same code path is what the feature builds.
#![cfg_attr(not(feature = "onnx"), allow(dead_code))]

use crate::Error;
use crate::task::ResultKind;

/// Where a tile's kept pixels are copied from and to.
#[derive(Clone, Copy, Debug)]
pub struct Blit {
    pub x: u32,
    pub y: u32,
    pub sx: u32,
    pub sy: u32,
    pub width: u32,
    pub height: u32,
}

/// Row-major RGB, three `f32` samples per pixel, in 0–1.
#[derive(Clone, Debug, PartialEq)]
pub struct RgbImage {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<f32>,
}

impl RgbImage {
    pub fn new(width: u32, height: u32) -> Result<Self, Error> {
        let n = sample_count(width, height)?;
        Ok(Self { width, height, rgb: vec![0.0; n] })
    }

    pub fn from_rgb(width: u32, height: u32, rgb: Vec<f32>) -> Result<Self, Error> {
        let n = sample_count(width, height)?;
        if rgb.len() != n {
            return Err(Error::Model(format!("RGB buffer has {} samples, expected {n}", rgb.len())));
        }
        Ok(Self { width, height, rgb })
    }

    pub fn pixel_count(&self) -> usize {
        (self.width as usize).saturating_mul(self.height as usize)
    }

    pub fn get(&self, x: u32, y: u32) -> [f32; 3] {
        let i = index(self.width, x, y);
        if i.saturating_add(2) >= self.rgb.len() {
            return [0.0; 3];
        }
        [self.rgb[i], self.rgb[i + 1], self.rgb[i + 2]]
    }

    pub fn set(&mut self, x: u32, y: u32, rgb: [f32; 3]) {
        let i = index(self.width, x, y);
        if i.saturating_add(2) >= self.rgb.len() {
            return;
        }
        self.rgb[i] = rgb[0];
        self.rgb[i + 1] = rgb[1];
        self.rgb[i + 2] = rgb[2];
    }

    /// A rectangle inside this image. `x + width` and `y + height` must stay inside.
    pub fn crop(&self, x: u32, y: u32, width: u32, height: u32) -> Result<Self, Error> {
        if width == 0 || height == 0 || x.saturating_add(width) > self.width || y.saturating_add(height) > self.height {
            return Err(Error::Model("tile crop is outside the image".into()));
        }
        let mut out = Self::new(width, height)?;
        let row = (width as usize).saturating_mul(3);
        for dy in 0..height {
            let src = index(self.width, x, y.saturating_add(dy));
            let dst = index(width, 0, dy);
            let Some(from) = self.rgb.get(src..src.saturating_add(row)) else { return Err(Error::Model("tile crop is outside the image".into())) };
            let Some(into) = out.rgb.get_mut(dst..dst.saturating_add(row)) else { return Err(Error::Model("tile crop is outside the image".into())) };
            into.copy_from_slice(from);
        }
        Ok(out)
    }

    /// Copy `copy.width`×`copy.height` samples from `src` at `(copy.sx, copy.sy)` onto `(copy.x, copy.y)`.
    pub fn blit(&mut self, src: &Self, copy: Blit) -> Result<(), Error> {
        if copy.width == 0 || copy.height == 0 {
            return Ok(());
        }
        if copy.x.saturating_add(copy.width) > self.width
            || copy.y.saturating_add(copy.height) > self.height
            || copy.sx.saturating_add(copy.width) > src.width
            || copy.sy.saturating_add(copy.height) > src.height
        {
            return Err(Error::Model("tile blit is outside the image".into()));
        }
        let row = (copy.width as usize).saturating_mul(3);
        for dy in 0..copy.height {
            let from = index(src.width, copy.sx, copy.sy.saturating_add(dy));
            let into = index(self.width, copy.x, copy.y.saturating_add(dy));
            let Some(src_row) = src.rgb.get(from..from.saturating_add(row)) else { return Err(Error::Model("tile blit is outside the image".into())) };
            let Some(dst_row) = self.rgb.get_mut(into..into.saturating_add(row)) else {
                return Err(Error::Model("tile blit is outside the image".into()));
            };
            dst_row.copy_from_slice(src_row);
        }
        Ok(())
    }
}

/// Pixels for the new layer.
///
/// `Absolute` keeps the model RGB. `Difference` is
/// `clamp(0.5 + 0.5 * (filtered - source), 0, 1)` per channel. Linear Light of that residual
/// over `source` is `filtered` when both images are in 0–1.
pub fn layer_pixels(kind: ResultKind, source: &RgbImage, mut filtered: RgbImage) -> Result<RgbImage, Error> {
    match kind {
        ResultKind::Absolute => Ok(filtered),
        ResultKind::Difference => {
            if source.width != filtered.width || source.height != filtered.height || source.rgb.len() != filtered.rgb.len() {
                return Err(Error::Model("filtered image size does not match the source".into()));
            }
            for (slot, src) in filtered.rgb.iter_mut().zip(source.rgb.iter()) {
                *slot = residual(*slot, *src);
            }
            Ok(filtered)
        }
    }
}

/// Mid-gray residual. NaN becomes 0.5, which Linear Light leaves unchanged. Infinity clamps to 0 or 1.
fn residual(filtered: f32, source: f32) -> f32 {
    let value = 0.5 + 0.5 * (filtered - source);
    if value.is_nan() { 0.5 } else { value.clamp(0.0, 1.0) }
}

pub(crate) fn sample_count(width: u32, height: u32) -> Result<usize, Error> {
    const MAX_PIXELS: u64 = 48_000_000;
    let pixels = u64::from(width).checked_mul(u64::from(height)).ok_or(Error::TooLarge)?;
    if pixels == 0 || pixels > MAX_PIXELS {
        return Err(Error::TooLarge);
    }
    let samples = pixels.checked_mul(3).ok_or(Error::TooLarge)?;
    usize::try_from(samples).map_err(|_| Error::TooLarge)
}

fn index(width: u32, x: u32, y: u32) -> usize {
    (y as usize).saturating_mul(width as usize).saturating_mul(3).saturating_add((x as usize).saturating_mul(3))
}

/// Bilinear resize. A constant image survives a round trip unchanged.
pub fn resize(src: &RgbImage, width: u32, height: u32) -> Result<RgbImage, Error> {
    if width == src.width && height == src.height {
        return Ok(src.clone());
    }
    if width == 0 || height == 0 || src.width == 0 || src.height == 0 {
        return Err(Error::Model("model spatial size is zero".into()));
    }
    let mut dst = RgbImage::new(width, height)?;
    for y in 0..height {
        let sy = (f64::from(y) + 0.5) * f64::from(src.height) / f64::from(height) - 0.5;
        for x in 0..width {
            let sx = (f64::from(x) + 0.5) * f64::from(src.width) / f64::from(width) - 0.5;
            dst.set(x, y, sample_bilinear(src, sx, sy));
        }
    }
    Ok(dst)
}

fn sample_bilinear(src: &RgbImage, x: f64, y: f64) -> [f32; 3] {
    let x = x.clamp(0.0, f64::from(src.width.saturating_sub(1)));
    let y = y.clamp(0.0, f64::from(src.height.saturating_sub(1)));
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(src.width.saturating_sub(1));
    let y1 = (y0 + 1).min(src.height.saturating_sub(1));
    let tx = (x - f64::from(x0)) as f32;
    let ty = (y - f64::from(y0)) as f32;
    let a = src.get(x0, y0);
    let b = src.get(x1, y0);
    let c = src.get(x0, y1);
    let d = src.get(x1, y1);
    let mut out = [0.0; 3];
    for i in 0..3 {
        let top = a[i] + (b[i] - a[i]) * tx;
        let bot = c[i] + (d[i] - c[i]) * tx;
        out[i] = top + (bot - top) * ty;
    }
    out
}

pub(crate) fn sanitize(image: &mut RgbImage) {
    for sample in &mut image.rgb {
        *sample = if sample.is_finite() { sample.clamp(0.0, 1.0) } else { 0.0 };
    }
}

/// NCHW packed `[1, 3, H, W]`.
pub(crate) fn to_nchw(image: &RgbImage) -> Vec<f32> {
    let n = image.pixel_count();
    let mut out = vec![0.0; n.saturating_mul(3)];
    if out.len() != image.rgb.len() {
        return Vec::new();
    }
    for i in 0..n {
        out[i] = image.rgb[i * 3];
        out[n + i] = image.rgb[i * 3 + 1];
        out[n * 2 + i] = image.rgb[i * 3 + 2];
    }
    out
}

/// `[1, 3, H, W]`, or one gray plane `[1, 1, H, W]` copied to R=G=B.
///
/// `0.5` stays `0.5` on every channel, which is neutral under Soft Light.
pub(crate) fn from_nchw(width: u32, height: u32, data: &[f32]) -> Result<RgbImage, Error> {
    let (pixels, n) = plane_len(width, height)?;
    if data.len() == pixels {
        return expand_gray(width, height, data);
    }
    if data.len() != n {
        return Err(Error::Model(format!("NCHW output has {} samples, expected {n} or {pixels}", data.len())));
    }
    let mut rgb = vec![0.0; n];
    for i in 0..pixels {
        let r = *data.get(i).ok_or_else(|| Error::Model("NCHW output is shorter than its shape".into()))?;
        let g = *data.get(pixels.saturating_add(i)).ok_or_else(|| Error::Model("NCHW output is shorter than its shape".into()))?;
        let b = *data.get(pixels.saturating_mul(2).saturating_add(i)).ok_or_else(|| Error::Model("NCHW output is shorter than its shape".into()))?;
        let base = i.saturating_mul(3);
        let slot = rgb.get_mut(base..base.saturating_add(3)).ok_or_else(|| Error::Model("NCHW output is shorter than its shape".into()))?;
        slot.copy_from_slice(&[r, g, b]);
    }
    finish(width, height, rgb)
}

/// `[1, H, W, 3]`, or one gray sample per pixel `[1, H, W, 1]` copied to R=G=B.
///
/// `0.5` stays `0.5` on every channel, which is neutral under Soft Light.
pub(crate) fn from_nhwc(width: u32, height: u32, data: &[f32]) -> Result<RgbImage, Error> {
    let (pixels, n) = plane_len(width, height)?;
    if data.len() == pixels {
        return expand_gray(width, height, data);
    }
    if data.len() != n {
        return Err(Error::Model(format!("NHWC output has {} samples, expected {n} or {pixels}", data.len())));
    }
    finish(width, height, data.to_vec())
}

fn plane_len(width: u32, height: u32) -> Result<(usize, usize), Error> {
    let rgb = sample_count(width, height)?;
    Ok((rgb / 3, rgb))
}

/// Copy a gray plane to R=G=B. One channel of NCHW and one channel of NHWC are the same row-major plane.
fn expand_gray(width: u32, height: u32, data: &[f32]) -> Result<RgbImage, Error> {
    let (pixels, n) = plane_len(width, height)?;
    if data.len() != pixels {
        return Err(Error::Model(format!("gray output has {} samples, expected {pixels}", data.len())));
    }
    let mut rgb = vec![0.0; n];
    for (i, sample) in data.iter().copied().enumerate() {
        let base = i.saturating_mul(3);
        let Some(slot) = rgb.get_mut(base..base.saturating_add(3)) else {
            return Err(Error::Model("gray output is outside the image".into()));
        };
        slot.fill(sample);
    }
    finish(width, height, rgb)
}

fn finish(width: u32, height: u32, rgb: Vec<f32>) -> Result<RgbImage, Error> {
    let mut image = RgbImage::from_rgb(width, height, rgb)?;
    sanitize(&mut image);
    Ok(image)
}

#[cfg(test)]
mod tests {
    use photocraft_color::BlendMode;
    use photocraft_color::blend::blend_channel;

    use super::{RgbImage, from_nchw, from_nhwc, layer_pixels, resize, to_nchw};
    use crate::task::ResultKind;

    #[test]
    fn a_constant_image_survives_a_resize_round_trip_and_both_layouts() {
        let mut image = RgbImage::new(2, 2).unwrap();
        image.rgb.fill(0.25);
        image.rgb[0] = f32::NAN;
        let up = resize(&image, 4, 4).unwrap();
        assert!(up.rgb.iter().any(|s| s.is_nan()));
        let packed = to_nchw(&image);
        let back = from_nchw(2, 2, &packed).unwrap();
        assert!(back.rgb.iter().all(|s| (*s - 0.25).abs() < 1e-6 || *s == 0.0));
        assert_eq!(back.rgb[0], 0.0, "non-finite samples become 0");
        let nhwc = from_nhwc(2, 2, &image.rgb).unwrap();
        assert_eq!(nhwc.rgb[0], 0.0);
        let restored = resize(&resize(&RgbImage::from_rgb(2, 2, vec![0.25; 12]).unwrap(), 4, 4).unwrap(), 2, 2).unwrap();
        assert!(restored.rgb.iter().all(|s| (*s - 0.25).abs() < 1e-5), "{:?}", restored.rgb);
    }

    #[test]
    fn one_channel_gray_expands_to_rgb_and_keeps_soft_light_neutral() {
        let nchw = from_nchw(2, 2, &[0.5, 0.2, 0.8, f32::NAN]).unwrap();
        assert_eq!(nchw.get(0, 0), [0.5, 0.5, 0.5]);
        assert_eq!(nchw.get(1, 0), [0.2, 0.2, 0.2]);
        assert_eq!(nchw.get(0, 1), [0.8, 0.8, 0.8]);
        assert_eq!(nchw.get(1, 1), [0.0, 0.0, 0.0], "non-finite gray becomes 0");
        let neutral = nchw.get(0, 0)[0];
        let base = 0.35;
        let soft = blend_channel(BlendMode::SoftLight, base, neutral);
        assert!((soft - base).abs() < 1e-5, "0.5 stays Soft Light neutral, got {soft}");

        let nhwc = from_nhwc(2, 1, &[0.5, 0.25]).unwrap();
        assert_eq!(nhwc.get(0, 0), [0.5, 0.5, 0.5]);
        assert_eq!(nhwc.get(1, 0), [0.25, 0.25, 0.25]);
        let rgb = from_nhwc(1, 1, &[0.1, 0.2, 0.3]).unwrap();
        assert_eq!(rgb.get(0, 0), [0.1, 0.2, 0.3], "three channels stay distinct");

        let err = from_nchw(2, 2, &[0.5; 5]).unwrap_err();
        assert!(err.to_string().contains("NCHW"), "{err}");
        let err = from_nhwc(2, 2, &[0.5; 5]).unwrap_err();
        assert!(err.to_string().contains("NHWC"), "{err}");
    }

    #[test]
    fn difference_residual_is_mid_gray_and_linear_light_reconstructs() {
        let mut source = RgbImage::new(1, 2).unwrap();
        source.set(0, 0, [0.2, 0.4, 0.8]);
        source.set(0, 1, [0.0, 1.0, 0.25]);
        let mut filtered = RgbImage::new(1, 2).unwrap();
        filtered.set(0, 0, [0.5, 0.4, 1.0]);
        filtered.set(0, 1, [0.0, 1.0, 0.25]);
        let layer = layer_pixels(ResultKind::Difference, &source, filtered.clone()).unwrap();
        let changed = layer.get(0, 0);
        assert!((changed[0] - 0.65).abs() < 1e-6, "{changed:?}");
        assert!((changed[1] - 0.5).abs() < 1e-6, "an unchanged channel is mid-gray");
        assert!((changed[2] - 0.6).abs() < 1e-6, "{changed:?}");
        let same = layer.get(0, 1);
        assert!(same.iter().all(|s| (*s - 0.5).abs() < 1e-6), "an unchanged pixel is mid-gray: {same:?}");
        for y in 0..2 {
            let src = source.get(0, y);
            let residual = layer.get(0, y);
            let want = filtered.get(0, y);
            for c in 0..3 {
                let got = blend_channel(BlendMode::LinearLight, src[c], residual[c]);
                assert!((got - want[c]).abs() < 1e-5, "y={y} c={c}: {got} vs {}", want[c]);
            }
        }
    }

    #[test]
    fn difference_residual_clamps_and_absolute_keeps_the_model_rgb() {
        let mut source = RgbImage::new(1, 1).unwrap();
        source.set(0, 0, [0.0, 1.0, 0.25]);
        let mut wild = RgbImage::new(1, 1).unwrap();
        wild.set(0, 0, [2.0, -1.0, f32::NAN]);
        let clamped = layer_pixels(ResultKind::Difference, &source, wild).unwrap();
        let px = clamped.get(0, 0);
        assert!((px[0] - 1.0).abs() < 1e-6, "{px:?}");
        assert!((px[1] - 0.0).abs() < 1e-6, "{px:?}");
        assert!((px[2] - 0.5).abs() < 1e-6, "a non-finite residual is mid-gray: {px:?}");
        let err = layer_pixels(ResultKind::Difference, &RgbImage::new(2, 1).unwrap(), RgbImage::new(1, 1).unwrap()).unwrap_err();
        assert!(err.to_string().contains("size"), "{err}");
        let mut filtered = RgbImage::new(1, 1).unwrap();
        filtered.rgb[0] = 1.25;
        filtered.rgb[1] = f32::NAN;
        let absolute = layer_pixels(ResultKind::Absolute, &source, filtered.clone()).unwrap();
        assert_eq!(absolute.rgb[0], 1.25);
        assert!(absolute.rgb[1].is_nan());
        assert_eq!(absolute.rgb[2], filtered.rgb[2]);
    }
}
