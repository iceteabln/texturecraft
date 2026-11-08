//! Image bytes for imported texture caches. Color stays 8-bit sRGB. Data maps are linear EXR
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
