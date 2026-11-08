//! Image bytes for imported the reference format caches. Color stays 8-bit sRGB. Data maps are linear EXR
//! so scalar channels are not gamma-decoded.

use std::io::Cursor;

/// RGBA8 PNG (sRGB). `rgba` is `width * height * 4`.
pub fn png_rgba8(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let img = image::RgbaImage::from_raw(width, height, rgba.to_vec()).ok_or_else(|| format!("base color is {width}x{height}, got {} bytes", rgba.len()))?;
    let mut png = Vec::new();
    img.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(png)
}

/// Linear OpenEXR, one value repeated into RGB (scalar mesh and material maps).
pub fn exr_gray(width: u32, height: u32, values: &[f32]) -> Result<Vec<u8>, String> {
    let n = (width as usize).saturating_mul(height as usize);
    if values.len() < n {
        return Err(format!("gray map is {width}x{height}, got {} values", values.len()));
    }
    let mut rgb = Vec::with_capacity(n * 3);
    for v in values.iter().take(n) {
        let v = if v.is_finite() { *v } else { 0.0 };
        rgb.extend([v, v, v]);
    }
    exr_rgb_raw(width, height, rgb)
}

/// Linear OpenEXR from interleaved RGB (`width * height * 3`).
pub fn exr_rgb(width: u32, height: u32, rgb: &[f32]) -> Result<Vec<u8>, String> {
    let n = (width as usize).saturating_mul(height as usize).saturating_mul(3);
    if rgb.len() < n {
        return Err(format!("rgb map is {width}x{height}, got {} values", rgb.len()));
    }
    exr_rgb_raw(width, height, rgb[..n].to_vec())
}

fn exr_rgb_raw(width: u32, height: u32, rgb: Vec<f32>) -> Result<Vec<u8>, String> {
    let img = image::Rgb32FImage::from_raw(width, height, rgb).ok_or_else(|| "exr buffer size".to_string())?;
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::OpenExr).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Color map from a bake host. the file's host writes TGA, which has no format signature, so the
/// bytes are decoded as TGA and re-encoded as PNG.
pub fn png_from_encoded(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let img = decode_baked(bytes)?.to_rgba8();
    png_rgba8(img.width(), img.height(), img.as_raw())
}

/// A baked archive map that is not a color: keep the file's 0..1 values, no sRGB decode.
pub fn exr_from_encoded(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let img = decode_baked(bytes)?;
    let (width, height) = (img.width(), img.height());
    let rgb = match &img {
        image::DynamicImage::ImageLuma8(b) => expand(b.pixels().map(|p| p.0[0] as f32 / 255.0)),
        image::DynamicImage::ImageLuma16(b) => expand(b.pixels().map(|p| p.0[0] as f32 / 65535.0)),
        image::DynamicImage::ImageLumaA8(b) => expand(b.pixels().map(|p| p.0[0] as f32 / 255.0)),
        image::DynamicImage::ImageLumaA16(b) => expand(b.pixels().map(|p| p.0[0] as f32 / 65535.0)),
        image::DynamicImage::ImageRgb8(b) => pack(b.pixels().map(|p| p.0.map(|c| c as f32 / 255.0))),
        image::DynamicImage::ImageRgba8(b) => pack(b.pixels().map(|p| [p.0[0], p.0[1], p.0[2]].map(|c| c as f32 / 255.0))),
        image::DynamicImage::ImageRgb16(b) => pack(b.pixels().map(|p| p.0.map(|c| c as f32 / 65535.0))),
        image::DynamicImage::ImageRgba16(b) => pack(b.pixels().map(|p| [p.0[0], p.0[1], p.0[2]].map(|c| c as f32 / 65535.0))),
        image::DynamicImage::ImageRgb32F(b) => pack(b.pixels().map(|p| p.0)),
        image::DynamicImage::ImageRgba32F(b) => pack(b.pixels().map(|p| [p.0[0], p.0[1], p.0[2]])),
        other => {
            let rgb = other.to_rgb8();
            pack(rgb.pixels().map(|p| p.0.map(|c| c as f32 / 255.0)))
        }
    };
    exr_rgb_raw(width, height, rgb)
}

fn decode_baked(bytes: &[u8]) -> Result<image::DynamicImage, String> {
    image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Tga)
        .decode()
        .or_else(|_| image::load_from_memory(bytes))
        .map_err(|e| format!("baked map: {e}"))
}

fn expand(values: impl Iterator<Item = f32>) -> Vec<f32> {
    values.flat_map(|v| [v, v, v]).collect()
}

fn pack(values: impl Iterator<Item = [f32; 3]>) -> Vec<f32> {
    values.flatten().collect()
}
