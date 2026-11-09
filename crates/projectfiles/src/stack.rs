//! Layer stack read from a project's `document.bin`.
#![allow(dead_code)]

use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct SppStackFile {
    pub format: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hbo_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_version: Option<u32>,
    pub materials: Vec<SppMaterialStack>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct SppMaterialStack {
    pub scene_material: String,
    pub resolution: u32,
    pub texture_sets: Vec<SppTextureSetStack>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct SppTextureSetStack {
    pub name: String,
    pub material_slot_name: String,
    pub resolution: u32,
    pub layers: Vec<SppLayerNode>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct SppLayerNode {
    pub kind: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default)]
    pub children: Vec<SppLayerNode>,
    #[serde(default)]
    pub node_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_color: Option<[f32; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roughness: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metallic: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_color_proc: Option<SppProceduralChannel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roughness_proc: Option<SppProceduralChannel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height_proc: Option<SppProceduralChannel>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub triplanar: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uv_scale: Option<f32>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub has_black_mask: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channel_blends: Vec<SppChannelBlend>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content_effects: Vec<SppContentEffect>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mask_effects: Vec<SppMaskEffect>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mask_decal_hits: Vec<SppDecalHit>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mask_paint_strokes: Vec<SppPaintStroke>,
    /// Filters stored on the layer (sharpen, blur, and the rest of the filter library).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content_filters: Vec<SppContentFilter>,
}

/// A filter the document applies to a layer, with the intensity it stored.
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct SppContentFilter {
    pub name: String,
    /// Filter library id (`sharpen`, `blur`, …).
    pub filter_id: String,
    pub amount: f32,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct SppProceduralChannel {
    pub pattern: String,
    #[serde(default = "default_seed")]
    pub seed: u32,
    pub scale: f32,
    pub balance: f32,
    pub contrast: f32,
    pub invert: bool,
    pub low: f32,
    pub high: f32,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct SppChannelBlend {
    pub channel: String,
    pub mode: String,
    pub opacity: f32,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct SppContentEffect {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_color: Option<[f32; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roughness: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metallic: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_color_proc: Option<SppProceduralChannel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roughness_proc: Option<SppProceduralChannel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height_proc: Option<SppProceduralChannel>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub triplanar: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uv_scale: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation_deg: Option<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channel_blends: Vec<SppChannelBlend>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct SppDecalHit {
    pub view_type: i64,
    pub granularity: i64,
    #[serde(default = "default_true")]
    pub backface_culling: bool,
    #[serde(default)]
    pub max_angle: f32,
    pub value: f32,
    #[serde(default)]
    pub is_rect: bool,
    #[serde(default)]
    pub pos: [f32; 2],
    #[serde(default)]
    pub rect: [f32; 4],
    pub proj_view_inv: [f32; 16],
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct SppPaintStroke {
    pub value: f32,
    pub size: f32,
    pub points: Vec<[f32; 3]>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct SppMaskEffect {
    pub name: String,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default = "default_blend")]
    pub blend: String,
    pub kind: SppMaskEffectKind,
}

/// Raw archive graph inputs preserved from a the reference format procedural fill (`urlToSbsRes` + tweaks).
#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
pub struct SppSbsarRef {
    /// the reference format resource URL, e.g. `/noise_cells_1/ratio_cells_1?...`.
    pub url: String,
    /// archive `scale` (or `x_amount` / tile) integer input.
    pub scale: i32,
    pub balance: f32,
    pub contrast: f32,
    pub disorder: f32,
    pub invert: bool,
    /// Seed tweak stored with the procedural fill.
    pub seed: i64,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SppMaskEffectKind {
    Generator {
        /// `"edges"`, `"cavity"`, `"directional"`, or `"position_gradient"`.
        generator: String,
        level: f32,
        softness: f32,
        invert: bool,
        noise_amount: f32,
        noise_scale: f32,
    },
    ProceduralFill {
        /// `"grunge"`, `"clouds"`, `"cells"`, `"scratches"`, `"perlin"`, `"wood_rings"`, `"stripes"`, `"checker"`, or `"brick"`.
        pattern: String,
        #[serde(default = "default_seed")]
        seed: u32,
        scale: f32,
        balance: f32,
        contrast: f32,
        invert: bool,
        triplanar: bool,
        /// the reference format `uvTransformation.scale` (tiling). Applied to `Projection.scale`, separate from
        /// the procedural pattern frequency above — matching the reference app's "Skalierung / Wiederholen".
        #[serde(default, skip_serializing_if = "Option::is_none")]
        uv_scale: Option<f32>,
        /// Original the reference format `urlToSbsRes` + raw archive graph inputs. When present, TextureCraft can
        /// bake the real `.sbsar` (Ratio Cells, BnW Spots, …) instead of approximating with Patterns.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sbsar: Option<SppSbsarRef>,
    },
    UniformFill {
        value: f32,
    },
    Levels {
        in_low: f32,
        in_high: f32,
        gamma: f32,
        out_low: f32,
        out_high: f32,
    },
    Invert,
    /// A filter library effect (`sharpen`, `blur`, …) with its stored intensity.
    Filter {
        filter_id: String,
        amount: f32,
    },
}

/// A graph URL TextureCraft can run with its own filter, or `None` when the URL is a generator,
/// a noise or a material fill.
pub fn filter_from_graph_url(url: &str) -> Option<&'static str> {
    let url = url.to_ascii_lowercase();
    if url.contains("mg_") || url.contains("grunge") || url.contains("noise") || url.contains("finish_") || url.contains("matfinish") || url.contains("cloud") {
        return None;
    }
    if url.contains("sharpen") {
        Some("sharpen")
    } else if url.contains("highpass") || url.contains("high_pass") {
        Some("highPass")
    } else if url.contains("slope_blur") || url.contains("blur_slope") {
        Some("slopeBlur")
    } else if url.contains("directional_blur") || url.contains("blur_directional") {
        Some("blurDirectional")
    } else if url.contains("hsl") || url.contains("hueshift") {
        Some("hsl")
    } else if url.contains("/blur") || url.ends_with("blur") {
        Some("blur")
    } else if url.contains("/invert") || url.ends_with("/invert") {
        Some("invert")
    } else {
        None
    }
}

fn default_seed() -> u32 {
    1
}

fn default_opacity() -> f32 {
    1.0
}

fn default_blend() -> String {
    "normal".into()
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn default_true() -> bool {
    true
}

pub fn load_stack(path: &Path) -> Result<SppStackFile, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("stack json: {e}"))
}

#[cfg(test)]
mod tests {
    use super::filter_from_graph_url;

    #[test]
    fn graph_urls_pick_filters_and_leave_generators() {
        assert_eq!(filter_from_graph_url("resource://shelf/fx_sharpen/sharpen"), Some("sharpen"));
        assert_eq!(filter_from_graph_url("resource://shelf/blur/blur"), Some("blur"));
        assert_eq!(filter_from_graph_url("resource://shelf/mg_metal_edge_wear/mg_metal_edge_wear"), None);
        assert_eq!(filter_from_graph_url("resource://shelf/finish_grainy/finish_grainy"), None);
        assert_eq!(filter_from_graph_url("resource://shelf/grunge_scratches/grunge_scratches"), None);
    }
}
