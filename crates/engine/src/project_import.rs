//! `.spp` projects, `.spsm` smart materials, `.spmsk` smart masks and `.sbsar` archives.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};
use texturecraft_core::{
    BakedMap, BlendMode, Channel, ChannelBlend, Effect, EffectKind, Fill, Filter, Generator, GeneratorKind, Layer, LayerId, LayerKind, Mask, MaskBackground,
    Procedural, ProceduralKind, Projection, Source,
};
use texturecraft_geom::glam::{Vec2, Vec3};
use texturecraft_geom::{Mesh, SubMesh};
use texturecraft_projectfiles::{
    CachedMaterialChannels, CachedMeshMap, SceneMapKind, SceneMesh, SppChannelBlend, SppContentEffect, SppLayerNode, SppMaskEffect, SppMaskEffectKind,
    SppProceduralChannel, SppStackFile,
};

use crate::commands::{CommandSpec, always, bad, strp};
use crate::materials::Place;
use crate::session::Session;
use crate::{EngineError, Result, cmd};

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "file.importProject",
        "Import project file",
        [],
        None,
        "{path: .spp project, .spsm smart material, .spmsk smart mask, or .sbsar generator or filter}",
        always,
        import_cmd
    )]
}

fn import_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let path = strp(p, "path").ok_or_else(|| bad("file.importProject", "`path` is required"))?;
    dispatch(s, path, Place::from_params(p))
}

/// Open a `.spp` project, or import a smart material, smart mask, filter or generator into the current one.
pub fn dispatch(s: &mut Session, path: &str, place: Place) -> Result<Value> {
    match ext_of(path).as_str() {
        "spp" => {
            let next = open_project(path)?;
            let summary = next.log.last().cloned().unwrap_or_else(|| format!("Opened {path}"));
            s.switch_to(next);
            Ok(json!({ "path": path, "kind": "project", "summary": summary }))
        }
        "spsm" | "spmsk" | "sbsar" => import_resource(s, path, place),
        other => Err(bad("file.importProject", format!(".{other} is not a project file (.spp, .spsm, .spmsk, .sbsar)"))),
    }
}

fn ext_of(path: &str) -> String {
    Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

fn stem_of(path: &str) -> String {
    Path::new(path).file_stem().and_then(|s| s.to_str()).unwrap_or("Project").to_string()
}

/// A new session from a `.spp` (mesh, layer stack, generators, baked mesh maps, texture cache).
pub fn open_project(path: &str) -> Result<Session> {
    let file = Path::new(path);
    let container = texturecraft_projectfiles::read_container(file).map_err(EngineError::Other)?;
    let (stack, _, _) = texturecraft_projectfiles::build_stack_from_document(&container.document_bin).map_err(EngineError::Other)?;
    let scene = texturecraft_projectfiles::import_assbin(&container.scene_assbin).map_err(|e| EngineError::Other(format!("mesh: {e}")))?;
    let mesh = to_geom(&scene)?;
    let resolution = stack_resolution(&stack);
    let obj = texturecraft_formats::obj_bytes(&mesh, &scene.name);
    let mut session = Session::new_project(&stem_of(path), mesh, "mesh.obj", Some(("obj".into(), obj)), resolution)?;
    session.doc.project.mesh_file = "scene.obj".into();
    let mut notes = Vec::new();
    import_stack(&mut session, &stack, &mut notes)?;
    match texturecraft_projectfiles::decode_scene_maps(file, resolution) {
        Ok(maps) => notes.push(format!("mesh maps: {}", import_mesh_maps(&mut session, &maps))),
        Err(e) => notes.push(format!("mesh maps skipped: {e}")),
    }
    match texturecraft_projectfiles::decode_texture_cache(file, resolution) {
        Ok(caches) => {
            let n = import_texture_cache(&mut session, &caches)?;
            if n > 0 {
                notes.push(format!("texture cache on {n} texture set(s) — hide \"Texture cache\" to edit the stack underneath"));
            }
        }
        Err(e) => notes.push(format!("texture cache skipped: {e}")),
    }
    session.sync_images();
    for cache in &mut session.caches {
        cache.invalidate_all();
    }
    session.modified = true;
    session.log.push(format!("Opened project {} ({})", file.display(), notes.join("; ")));
    Ok(session)
}

fn import_resource(s: &mut Session, path: &str, place: Place) -> Result<Value> {
    let file = Path::new(path);
    match ext_of(path).as_str() {
        "spsm" => {
            let node = texturecraft_projectfiles::decode_spsm(file).map_err(EngineError::Other)?;
            let id = s.edit("Import smart material", |s| insert_root(s, &node, place))?;
            s.sync_images();
            Ok(json!({ "path": path, "kind": "smartMaterial", "layer": id.0, "name": node.name }))
        }
        "spmsk" => {
            let effects = texturecraft_projectfiles::decode_spmsk(file).map_err(EngineError::Other)?;
            if effects.is_empty() {
                return Err(bad("file.importProject", format!("{path}: no mask generators or filters in this smart mask")));
            }
            let layer = s.selected_layer().ok_or_else(|| bad("file.importProject", "select a layer to apply the smart mask"))?;
            let n = effects.len();
            s.edit("Import smart mask", |s| {
                apply_mask_effects(s, layer, &effects)?;
                Ok(())
            })?;
            Ok(json!({ "path": path, "kind": "smartMask", "layer": layer.0, "effects": n }))
        }
        "sbsar" => import_sbsar(s, path, place),
        _ => Err(bad("file.importProject", "unsupported project file")),
    }
}

fn import_sbsar(s: &mut Session, path: &str, place: Place) -> Result<Value> {
    let name = stem_of(path);
    let kind = archive_kind(path, &name);
    match kind {
        ArchiveKind::Generator(generator) => {
            let id = s.edit("Import generator", |s| {
                let layer = s.selected_layer();
                if let Some(layer) = layer {
                    let effect = generator_effect(s, &name, generator);
                    let mask = s.doc.project.layer_mut(layer)?.mask.get_or_insert_with(|| Mask::new(MaskBackground::White));
                    mask.effects.push(effect);
                    Ok(layer)
                } else {
                    let mut node = empty_fill(&name);
                    node.has_black_mask = false;
                    node.mask_effects.push(generator_mask(&name, generator));
                    insert_root(s, &node, place)
                }
            })?;
            Ok(json!({ "path": path, "kind": "generator", "layer": id.0, "name": name }))
        }
        ArchiveKind::Filter(filter_id) => {
            let effect_kind = crate::filter_lib::build(filter_id, &Value::Null)?;
            let id = s.edit("Import filter", |s| {
                if let Some(layer) = s.selected_layer() {
                    let effect = Effect::new(s.doc.project.alloc_id(), effect_kind);
                    s.doc.project.layer_mut(layer)?.content_effects.push(effect);
                    Ok(layer)
                } else {
                    let node = empty_fill(&name);
                    let id = insert_root(s, &node, place)?;
                    let effect = Effect::new(s.doc.project.alloc_id(), effect_kind);
                    s.doc.project.layer_mut(id)?.content_effects.push(effect);
                    Ok(id)
                }
            })?;
            Ok(json!({ "path": path, "kind": "filter", "layer": id.0, "name": name, "filter": filter_id }))
        }
        ArchiveKind::Material => Err(bad(
            "file.importProject",
            format!(
                "{path} is a compiled material graph. Open the .spp project or the .spsm smart material; those carry the live layer stack. Generators and known filters import from .sbsar directly."
            ),
        )),
    }
}

enum ArchiveKind {
    Generator(GeneratorKind),
    Filter(&'static str),
    Material,
}

fn archive_kind(path: &str, name: &str) -> ArchiveKind {
    let parent = Path::new(path).parent().and_then(|p| p.file_name()).and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    let key = name.to_ascii_lowercase().replace([' ', '-'], "_");
    if parent == "generators" || parent == "generator" {
        return ArchiveKind::Generator(generator_kind_from_name(&key).unwrap_or(GeneratorKind::Curvature));
    }
    if let Some(kind) = generator_kind_from_name(&key)
        && (key.starts_with("mg_") || key.contains("edge_wear") || key.contains("curvature") || key.contains("ambient_occlusion"))
    {
        return ArchiveKind::Generator(kind);
    }
    if (parent == "effects" || parent == "finishes" || parent == "filters")
        && let Some(id) = filter_id_from_name(&key)
    {
        return ArchiveKind::Filter(id);
    }
    if let Some(id) = filter_id_from_name(&key)
        && (key.starts_with("matfinish") || key.starts_with("matfx") || key.contains("blur") || key.contains("hsl") || key.contains("level"))
    {
        return ArchiveKind::Filter(id);
    }
    ArchiveKind::Material
}

fn generator_kind_from_name(key: &str) -> Option<GeneratorKind> {
    if key.contains("edge_wear") || key.contains("mask_editor") || key.contains("mask_builder") {
        Some(GeneratorKind::EdgeWear)
    } else if key.contains("dirt") || key.contains("dripping_rust") || key.contains("cavity") {
        Some(GeneratorKind::Dirt)
    } else if key.contains("ambient_occlusion") || key.contains("hbao") || key.ends_with("_ao") {
        Some(GeneratorKind::AmbientOcclusion)
    } else if key.contains("curvature") {
        Some(GeneratorKind::Curvature)
    } else if key.contains("thickness") || key.contains("thin") {
        Some(GeneratorKind::ThinAreas)
    } else if key.contains("position") || key.contains("linear_gradient") || key.contains("distance") {
        Some(GeneratorKind::Position)
    } else if key.contains("world_space_normal") || key == "light" || key.contains("direction") {
        Some(GeneratorKind::Direction)
    } else {
        None
    }
}

fn filter_id_from_name(key: &str) -> Option<&'static str> {
    if key.contains("hammer") {
        Some("matFinishHammered")
    } else if key.contains("grind") || key.contains("brushed") {
        Some("matFinishGrind")
    } else if key.contains("sand") {
        Some("matFinishSanded")
    } else if key.contains("matfinish_raw") || key.contains("raw") && key.contains("finish") {
        Some("matFinishRaw")
    } else if key.contains("histogram_scan") {
        Some("histogramScan")
    } else if key.contains("histogram_range") {
        Some("histogramRange")
    } else if key.contains("histogram_shift") || key.contains("histogram_position") {
        Some("histogramShift")
    } else if key.contains("contrast") {
        Some("contrastLuminosity")
    } else if key.contains("hsl") || key.contains("hueshift") {
        Some("hsl")
    } else if key.contains("slope_blur") || key.contains("blur_slope") {
        Some("slopeBlur")
    } else if key.contains("directional_blur") || key.contains("blur_directional") {
        Some("blurDirectional")
    } else if key.contains("highpass") || key.contains("high_pass") {
        Some("highPass")
    } else if key.contains("sharpen") {
        Some("sharpen")
    } else if key.contains("bevel") {
        Some("bevel")
    } else if key.contains("mosaic") {
        Some("mosaic")
    } else if key.contains("glow") {
        Some("glow")
    } else if key.contains("warp") {
        Some("warp")
    } else if key.contains("blur") {
        Some("blur")
    } else if key.contains("invert") {
        Some("invert")
    } else if key.contains("grayscale") || key.contains("grey") {
        Some("grayscale")
    } else if key.contains("level") {
        Some("levels")
    } else {
        None
    }
}

fn empty_fill(name: &str) -> SppLayerNode {
    SppLayerNode {
        kind: "fill".into(),
        name: name.into(),
        visible: true,
        children: Vec::new(),
        node_kind: None,
        base_color: None,
        roughness: None,
        metallic: None,
        height: None,
        base_color_proc: None,
        roughness_proc: None,
        height_proc: None,
        triplanar: false,
        uv_scale: None,
        has_black_mask: false,
        channel_blends: Vec::new(),
        content_effects: Vec::new(),
        mask_effects: Vec::new(),
        mask_decal_hits: Vec::new(),
        mask_paint_strokes: Vec::new(),
    }
}

fn generator_mask(name: &str, kind: GeneratorKind) -> SppMaskEffect {
    SppMaskEffect {
        name: name.into(),
        opacity: 1.0,
        blend: "normal".into(),
        kind: SppMaskEffectKind::Generator {
            generator: match kind {
                GeneratorKind::EdgeWear => "edges",
                GeneratorKind::Dirt => "cavity",
                GeneratorKind::Direction => "directional",
                GeneratorKind::Position => "position_gradient",
                GeneratorKind::AmbientOcclusion => "ao",
                GeneratorKind::ThinAreas => "thickness",
                GeneratorKind::Curvature => "curvature",
            }
            .into(),
            level: 0.5,
            softness: 0.25,
            invert: false,
            noise_amount: 0.0,
            noise_scale: 8.0,
        },
    }
}

fn generator_effect(s: &mut Session, name: &str, kind: GeneratorKind) -> Effect {
    let mut effect = Effect::new(s.doc.project.alloc_id(), EffectKind::Generator(Generator::new(kind)));
    effect.name = name.to_string();
    effect
}

fn insert_root(s: &mut Session, node: &SppLayerNode, place: Place) -> Result<LayerId> {
    let ts = s.ts();
    let channels = s.doc.project.texture_set(ts)?.channel_list();
    let layer = build_layer(&mut s.doc.project, &channels, node)?;
    let id = crate::layer_cmds::insert_selected(s, layer)?;
    push_children(&mut s.doc.project, ts, Some(id), &node.children, &channels)?;
    place.apply(s, id)?;
    s.selected.insert(ts, id);
    Ok(id)
}

fn import_stack(s: &mut Session, stack: &SppStackFile, notes: &mut Vec<String>) -> Result<()> {
    let mut imported = 0usize;
    for (index, set) in s.doc.project.texture_sets.clone().iter().enumerate() {
        let Some(material) = stack
            .materials
            .iter()
            .find(|m| m.scene_material.eq_ignore_ascii_case(&set.name))
            .or_else(|| (stack.materials.len() == s.doc.project.texture_sets.len()).then(|| stack.materials.get(index)).flatten())
        else {
            continue;
        };
        let layers = material.texture_sets.first().map(|t| t.layers.as_slice()).unwrap_or(&[]);
        let ids = s.doc.project.texture_sets[index].stack.clone();
        for id in ids {
            let _ = s.doc.project.remove_layer(id);
        }
        let channels = s.doc.project.texture_set(index)?.channel_list();
        push_children(&mut s.doc.project, index, None, layers, &channels)?;
        imported += count_nodes(layers);
        if let Some(top) = s.doc.project.texture_sets[index].stack.last().copied() {
            s.selected.insert(index, top);
        }
    }
    notes.push(format!("{imported} layers"));
    if imported == 0 {
        notes.push("no layer stack matched the mesh materials".into());
    }
    Ok(())
}

fn push_children(project: &mut texturecraft_core::Project, ts: usize, parent: Option<LayerId>, nodes: &[SppLayerNode], channels: &[Channel]) -> Result<()> {
    for node in nodes {
        let layer = build_layer(project, channels, node)?;
        let id = match parent {
            None => project.insert_layer(ts, layer, None, false)?,
            Some(parent) => project.insert_layer(ts, layer, Some(parent), true)?,
        };
        if node.kind == "folder" {
            push_children(project, ts, Some(id), &node.children, channels)?;
        }
    }
    Ok(())
}

fn build_layer(project: &mut texturecraft_core::Project, channels: &[Channel], node: &SppLayerNode) -> Result<Layer> {
    let id = project.alloc_id();
    let kind = if node.kind == "folder" {
        LayerKind::Group { children: Vec::new(), collapsed: false }
    } else if node.kind == "paint" && !has_fill(node) {
        LayerKind::Paint
    } else {
        LayerKind::Fill(fill_from_node(node))
    };
    let mut layer = Layer::new(id, &node.name, kind, channels);
    layer.visible = node.visible;
    apply_blends(&mut layer, node);
    if let Some(mask) = mask_from(project, node) {
        layer.mask = Some(mask);
    }
    layer.content_effects = content_from(project, node);
    if matches!(layer.kind, LayerKind::Fill(_)) {
        let used = used_channels(node);
        if !used.is_empty() {
            layer.channels.retain(|c, _| used.contains(c));
        }
    }
    Ok(layer)
}

struct FillBits<'a> {
    base: Option<[f32; 3]>,
    roughness: Option<f32>,
    metallic: Option<f32>,
    height: Option<f32>,
    base_proc: Option<&'a SppProceduralChannel>,
    rough_proc: Option<&'a SppProceduralChannel>,
    height_proc: Option<&'a SppProceduralChannel>,
}

fn node_bits(node: &SppLayerNode) -> FillBits<'_> {
    FillBits {
        base: node.base_color,
        roughness: node.roughness,
        metallic: node.metallic,
        height: node.height,
        base_proc: node.base_color_proc.as_ref(),
        rough_proc: node.roughness_proc.as_ref(),
        height_proc: node.height_proc.as_ref(),
    }
}

fn effect_bits(effect: &SppContentEffect) -> FillBits<'_> {
    FillBits {
        base: effect.base_color,
        roughness: effect.roughness,
        metallic: effect.metallic,
        height: effect.height,
        base_proc: effect.base_color_proc.as_ref(),
        rough_proc: effect.roughness_proc.as_ref(),
        height_proc: effect.height_proc.as_ref(),
    }
}

fn used_channels(node: &SppLayerNode) -> BTreeSet<Channel> {
    let mut used = BTreeSet::new();
    collect_fill_channels(&node_bits(node), &mut used);
    for effect in &node.content_effects {
        collect_fill_channels(&effect_bits(effect), &mut used);
    }
    for blend in &node.channel_blends {
        if let Some(channel) = channel_of(&blend.channel) {
            used.insert(channel);
        }
    }
    used
}

fn collect_fill_channels(bits: &FillBits<'_>, used: &mut BTreeSet<Channel>) {
    if bits.base.is_some() || bits.base_proc.is_some() {
        used.insert(Channel::BaseColor);
    }
    if bits.roughness.is_some() || bits.rough_proc.is_some() {
        used.insert(Channel::Roughness);
    }
    if bits.metallic.is_some() {
        used.insert(Channel::Metallic);
    }
    if bits.height.is_some() || bits.height_proc.is_some() {
        used.insert(Channel::Height);
    }
}

fn has_fill(node: &SppLayerNode) -> bool {
    node.base_color.is_some()
        || node.roughness.is_some()
        || node.metallic.is_some()
        || node.height.is_some()
        || node.base_color_proc.is_some()
        || node.roughness_proc.is_some()
        || node.height_proc.is_some()
        || !node.content_effects.is_empty()
}

fn fill_from_node(node: &SppLayerNode) -> Fill {
    let mut fill = Fill { projection: projection(node.triplanar, node.uv_scale, None), ..Fill::default() };
    write_sources(&mut fill, &node_bits(node));
    fill
}

fn content_fill(effect: &SppContentEffect) -> Fill {
    let mut fill = Fill { projection: projection(effect.triplanar, effect.uv_scale, effect.rotation_deg), ..Fill::default() };
    write_sources(&mut fill, &effect_bits(effect));
    fill
}

fn write_sources(fill: &mut Fill, bits: &FillBits<'_>) {
    if let Some(proc) = bits.base_proc {
        fill.sources.insert(Channel::BaseColor, proc_source(proc));
    } else if let Some(color) = bits.base {
        fill.sources.insert(Channel::BaseColor, Source::Uniform(color));
    }
    if let Some(proc) = bits.rough_proc {
        fill.sources.insert(Channel::Roughness, proc_source(proc));
    } else if let Some(value) = bits.roughness {
        fill.sources.insert(Channel::Roughness, Source::Uniform([value; 3]));
    }
    if let Some(value) = bits.metallic {
        fill.sources.insert(Channel::Metallic, Source::Uniform([value; 3]));
    }
    if let Some(proc) = bits.height_proc {
        fill.sources.insert(Channel::Height, proc_source(proc));
    } else if let Some(value) = bits.height {
        fill.sources.insert(Channel::Height, Source::Uniform([value; 3]));
    }
}

fn proc_source(proc: &SppProceduralChannel) -> Source {
    let mut pattern = Procedural::new(procedural_kind(&proc.pattern));
    pattern.scale = proc.scale.max(0.01);
    pattern.seed = proc.seed;
    pattern.balance = proc.balance;
    pattern.contrast = proc.contrast.max(0.01);
    pattern.invert = proc.invert;
    Source::Procedural { pattern, low: [proc.low; 3], high: [proc.high; 3] }
}

fn procedural_kind(pattern: &str) -> ProceduralKind {
    match pattern {
        "clouds" | "perlin" => ProceduralKind::Clouds,
        "cells" | "brick" => ProceduralKind::Cells,
        "scratches" => ProceduralKind::Scratches,
        "wood_rings" => ProceduralKind::Wood,
        "stripes" => ProceduralKind::Stripes,
        "checker" => ProceduralKind::Checker,
        "noise" => ProceduralKind::Noise,
        _ => ProceduralKind::Grunge,
    }
}

fn projection(triplanar: bool, uv_scale: Option<f32>, rotation: Option<f32>) -> Projection {
    let scale = uv_scale.unwrap_or(1.0).max(0.001);
    if triplanar {
        Projection::Triplanar { scale, sharpness: 4.0, offset: [0.0; 3] }
    } else {
        Projection::Uv { scale: [scale, scale], rotation: rotation.unwrap_or(0.0), offset: [0.0, 0.0] }
    }
}

fn apply_blends(layer: &mut Layer, node: &SppLayerNode) {
    for blend in &node.channel_blends {
        let mode = blend_mode(&blend.mode);
        let opacity = blend.opacity.clamp(0.0, 1.0);
        if blend.channel == "folder" && layer.is_group() {
            for slot in layer.channels.values_mut() {
                *slot = ChannelBlend { mode, opacity };
            }
            continue;
        }
        let Some(channel) = channel_of(&blend.channel) else { continue };
        if !layer.channels.contains_key(&channel) {
            continue;
        }
        let mode = if channel == Channel::Normal && mode == BlendMode::Normal { BlendMode::NormalMapCombine } else { mode };
        layer.channels.insert(channel, ChannelBlend { mode, opacity });
    }
    if matches!(layer.kind, LayerKind::Fill(_))
        && layer.channels.contains_key(&Channel::Height)
        && !node.channel_blends.iter().any(|b| b.channel == "height")
        && let Some(slot) = layer.channels.get_mut(&Channel::Height)
    {
        slot.mode = BlendMode::LinearDodge;
    }
}

fn mask_from(project: &mut texturecraft_core::Project, node: &SppLayerNode) -> Option<Mask> {
    if node.mask_effects.is_empty() && !node.has_black_mask {
        return None;
    }
    let mut mask = Mask::new(if node.has_black_mask { MaskBackground::Black } else { MaskBackground::White });
    for effect in &node.mask_effects {
        mask.effects.push(mask_effect(project, effect));
    }
    Some(mask)
}

fn apply_mask_effects(s: &mut Session, layer: LayerId, effects: &[SppMaskEffect]) -> Result<()> {
    let built: Vec<Effect> = effects.iter().map(|effect| mask_effect(&mut s.doc.project, effect)).collect();
    let mask = s.doc.project.layer_mut(layer)?.mask.get_or_insert_with(|| Mask::new(MaskBackground::White));
    mask.effects.extend(built);
    Ok(())
}

fn mask_effect(project: &mut texturecraft_core::Project, effect: &SppMaskEffect) -> Effect {
    let mut built = Effect::new(project.alloc_id(), mask_kind(&effect.kind));
    built.name = effect.name.clone();
    built.opacity = effect.opacity.clamp(0.0, 1.0);
    built.blend = blend_mode(&effect.blend);
    built
}

fn mask_kind(kind: &SppMaskEffectKind) -> EffectKind {
    match kind {
        SppMaskEffectKind::Generator { generator, level, softness, invert, noise_amount, noise_scale } => {
            let mut g = Generator::new(match generator.as_str() {
                "edges" => GeneratorKind::EdgeWear,
                "cavity" | "dirt" => GeneratorKind::Dirt,
                "directional" | "direction" => GeneratorKind::Direction,
                "position_gradient" | "position" => GeneratorKind::Position,
                "ao" | "ambient_occlusion" => GeneratorKind::AmbientOcclusion,
                "thickness" | "thin" => GeneratorKind::ThinAreas,
                _ => GeneratorKind::Curvature,
            });
            g.level = *level;
            g.softness = *softness;
            g.invert = *invert;
            g.noise_amount = *noise_amount;
            g.noise_scale = if *noise_scale > 0.0 { *noise_scale } else { g.noise_scale };
            EffectKind::Generator(g)
        }
        SppMaskEffectKind::ProceduralFill { pattern, seed, scale, balance, contrast, invert, triplanar, uv_scale, .. } => {
            let proc = SppProceduralChannel {
                pattern: pattern.clone(),
                seed: *seed,
                scale: *scale,
                balance: *balance,
                contrast: *contrast,
                invert: *invert,
                low: 0.0,
                high: 1.0,
            };
            EffectKind::Fill { source: proc_source(&proc), projection: projection(*triplanar, *uv_scale, None) }
        }
        SppMaskEffectKind::UniformFill { value } => EffectKind::Fill { source: Source::Uniform([*value; 3]), projection: Projection::default() },
        SppMaskEffectKind::Levels { in_low, in_high, gamma, out_low, out_high } => {
            EffectKind::Levels(texturecraft_core::Levels { in_low: *in_low, in_high: *in_high, gamma: *gamma, out_low: *out_low, out_high: *out_high })
        }
        SppMaskEffectKind::Invert => EffectKind::Filter(Filter::Invert),
    }
}

fn content_from(project: &mut texturecraft_core::Project, node: &SppLayerNode) -> Vec<Effect> {
    node.content_effects
        .iter()
        .map(|effect| {
            let (mode, opacity) = effect_blend(&effect.channel_blends);
            let mut built = Effect::new(project.alloc_id(), EffectKind::Material(content_fill(effect)));
            built.name = effect.name.clone();
            built.blend = mode;
            built.opacity = opacity;
            built
        })
        .collect()
}

fn effect_blend(blends: &[SppChannelBlend]) -> (BlendMode, f32) {
    let blend = blends.iter().find(|b| b.channel == "base_color").or_else(|| blends.first());
    match blend {
        Some(blend) => (blend_mode(&blend.mode), blend.opacity.clamp(0.0, 1.0)),
        None => (BlendMode::Normal, 1.0),
    }
}

fn blend_mode(name: &str) -> BlendMode {
    match name {
        "disable" => BlendMode::Disable,
        "passthrough" => BlendMode::Passthrough,
        "replace" => BlendMode::Replace,
        "multiply" => BlendMode::Multiply,
        "add" => BlendMode::LinearDodge,
        "subtract" => BlendMode::Subtract,
        "add_sub" => BlendMode::SignedAddition,
        "min" => BlendMode::Darken,
        "max" => BlendMode::Lighten,
        "overlay" => BlendMode::Overlay,
        "screen" => BlendMode::Screen,
        "soft_light" => BlendMode::SoftLight,
        "hard_light" => BlendMode::HardLight,
        "vivid_light" => BlendMode::VividLight,
        "linear_light" => BlendMode::LinearLight,
        "pin_light" => BlendMode::PinLight,
        "color_dodge" => BlendMode::ColorDodge,
        "color_burn" => BlendMode::ColorBurn,
        "linear_burn" => BlendMode::LinearBurn,
        "difference" => BlendMode::Difference,
        "exclusion" => BlendMode::Exclusion,
        "divide" => BlendMode::Divide,
        "inverse_divide" => BlendMode::InverseDivide,
        "inverse_subtract" => BlendMode::InverseSubtract,
        "color" => BlendMode::Color,
        "tint" => BlendMode::Tint,
        "saturation" => BlendMode::Saturation,
        "value" => BlendMode::Value,
        "normal_combine" => BlendMode::NormalMapCombine,
        "normal_detail" => BlendMode::NormalMapDetail,
        "normal_inverse_detail" => BlendMode::NormalMapInverseDetail,
        _ => BlendMode::Normal,
    }
}

fn channel_of(name: &str) -> Option<Channel> {
    match name {
        "base_color" | "basecolor" | "diffuse" => Some(Channel::BaseColor),
        "roughness" => Some(Channel::Roughness),
        "metallic" => Some(Channel::Metallic),
        "height" => Some(Channel::Height),
        "normal" => Some(Channel::Normal),
        "emissive" | "emission" => Some(Channel::Emissive),
        "opacity" => Some(Channel::Opacity),
        _ => None,
    }
}

fn import_mesh_maps(s: &mut Session, maps: &[CachedMeshMap]) -> usize {
    let mut applied = 0usize;
    for map in maps {
        let Some(kind) = baked_kind(map.kind) else { continue };
        let Some(ts) = s.doc.project.texture_sets.iter().position(|set| set.name.eq_ignore_ascii_case(&map.material_name)) else { continue };
        let Some(data) = pack_map(kind, map) else { continue };
        let entry = s.doc.baked.entry(ts).or_default();
        if entry.maps.is_empty() {
            entry.resolution = map.width;
        }
        if entry.resolution != map.width {
            continue;
        }
        entry.maps.insert(kind, Arc::new(data));
        if let Ok(set) = s.doc.project.texture_set_mut(ts) {
            set.baked.insert(kind);
        }
        applied += 1;
    }
    applied
}

fn baked_kind(kind: SceneMapKind) -> Option<BakedMap> {
    Some(match kind {
        SceneMapKind::Normal => BakedMap::Normal,
        SceneMapKind::WorldSpaceNormal => BakedMap::WorldNormal,
        SceneMapKind::Id => BakedMap::Id,
        SceneMapKind::AmbientOcclusion => BakedMap::AmbientOcclusion,
        SceneMapKind::Curvature => BakedMap::Curvature,
        SceneMapKind::Position => BakedMap::Position,
        SceneMapKind::Thickness => BakedMap::Thickness,
    })
}

fn pack_map(kind: BakedMap, map: &CachedMeshMap) -> Option<Vec<f32>> {
    let texels = (map.width as usize).saturating_mul(map.height as usize);
    let src = map.channels as usize;
    if src == 0 || map.pixels_f32.len() < texels.saturating_mul(src) {
        return None;
    }
    let comps = kind.components();
    let mut out = Vec::with_capacity(texels.saturating_mul(comps));
    for i in 0..texels {
        let base = i * src;
        if comps == 1 {
            out.push(map.pixels_f32[base]);
        } else if src >= 3 {
            out.extend_from_slice(&map.pixels_f32[base..base + 3]);
        } else {
            out.extend(std::iter::repeat_n(map.pixels_f32[base], 3));
        }
    }
    Some(out)
}

fn import_texture_cache(s: &mut Session, caches: &[CachedMaterialChannels]) -> Result<usize> {
    let mut applied = 0usize;
    for cache in caches {
        let Some(ts) = s.doc.project.texture_sets.iter().position(|set| set.name.eq_ignore_ascii_case(&cache.material_name)) else { continue };
        let channels = s.doc.project.texture_set(ts)?.channel_list();
        let mut fill = Fill::default();
        let mut enabled = BTreeSet::new();
        let stem = cache.material_name.replace(['\\', '/', ':'], "_");
        if let Some(rgba) = &cache.base_color_rgba8 {
            let name = format!("{stem}_BaseColor.png");
            let png = texturecraft_projectfiles::pixels::png_rgba8(cache.width, cache.height, rgba).map_err(EngineError::Other)?;
            let name = s.store_resource_bytes(&name, png)?;
            fill.sources.insert(Channel::BaseColor, Source::image(name));
            enabled.insert(Channel::BaseColor);
        }
        if let Some(ga) = &cache.roughness_ga8 {
            let values = ga.chunks(2).map(|c| c[0] as f32 / 255.0).collect::<Vec<_>>();
            add_gray(s, &mut fill, &mut enabled, Channel::Roughness, &format!("{stem}_Roughness.exr"), (cache.width, cache.height), &values)?;
        }
        if let Some(ga) = &cache.metallic_ga8 {
            let values = ga.chunks(2).map(|c| c[0] as f32 / 255.0).collect::<Vec<_>>();
            add_gray(s, &mut fill, &mut enabled, Channel::Metallic, &format!("{stem}_Metallic.exr"), (cache.width, cache.height), &values)?;
        }
        if let Some(height) = &cache.height_f32 {
            // The cache stores signed height (0 is flat). A signed channel reads an image as value * 2 - 1.
            let encoded: Vec<f32> = height.iter().map(|v| (if v.is_finite() { *v } else { 0.0 }) * 0.5 + 0.5).collect();
            add_gray(s, &mut fill, &mut enabled, Channel::Height, &format!("{stem}_Height.exr"), (cache.width, cache.height), &encoded)?;
        }
        if let Some(normal) = &cache.normal_rgb_f32 {
            let name = format!("{stem}_Normal.exr");
            let exr = texturecraft_projectfiles::pixels::exr_rgb(cache.width, cache.height, normal).map_err(EngineError::Other)?;
            let name = s.store_resource_bytes(&name, exr)?;
            fill.sources.insert(Channel::Normal, Source::image(name));
            enabled.insert(Channel::Normal);
        }
        enabled.retain(|c| channels.contains(c));
        fill.sources.retain(|c, _| enabled.contains(c));
        if enabled.is_empty() {
            continue;
        }
        let id = s.doc.project.alloc_id();
        let mut layer = Layer::new(id, "Texture cache", LayerKind::Fill(fill), &channels);
        layer.channels.retain(|c, _| enabled.contains(c));
        s.doc.project.insert_layer(ts, layer, None, false)?;
        s.selected.insert(ts, id);
        applied += 1;
    }
    Ok(applied)
}

fn add_gray(s: &mut Session, fill: &mut Fill, enabled: &mut BTreeSet<Channel>, channel: Channel, name: &str, size: (u32, u32), values: &[f32]) -> Result<()> {
    let exr = texturecraft_projectfiles::pixels::exr_gray(size.0, size.1, values).map_err(EngineError::Other)?;
    let name = s.store_resource_bytes(name, exr)?;
    fill.sources.insert(channel, Source::image(name));
    enabled.insert(channel);
    Ok(())
}

fn stack_resolution(stack: &SppStackFile) -> u32 {
    let cap = std::env::var("TEXTURECRAFT_SPP_MAX_RESOLUTION").ok().and_then(|v| v.parse().ok()).unwrap_or(2048);
    let raw = stack.materials.iter().map(|m| m.resolution).find(|r| *r >= 128).unwrap_or(2048);
    let cap = cap.clamp(128, 8192);
    let raw = raw.clamp(128, cap);
    if raw.is_power_of_two() { raw } else { (raw.next_power_of_two() / 2).clamp(128, cap) }
}

fn to_geom(scene: &SceneMesh) -> Result<Mesh> {
    let mut mesh = Mesh {
        positions: scene.positions.iter().copied().map(Vec3::from).collect(),
        normals: scene.normals.iter().copied().map(Vec3::from).collect(),
        uvs: scene.uvs.iter().copied().map(Vec2::from).collect(),
        indices: scene.indices.clone(),
        materials: scene.materials.clone(),
        submeshes: scene
            .parts
            .iter()
            .map(|(name, material, start, count)| SubMesh { name: name.clone(), material: *material, start: *start, count: *count })
            .collect(),
        ..Mesh::default()
    };
    if mesh.materials.is_empty() {
        mesh.materials.push("Default".into());
    }
    if mesh.submeshes.is_empty() {
        mesh.submeshes.push(SubMesh { name: mesh.materials[0].clone(), material: 0, start: 0, count: mesh.indices.len() as u32 });
    }
    if mesh.normals.len() != mesh.positions.len() {
        mesh.compute_normals();
    }
    if mesh.uvs.len() != mesh.positions.len() {
        mesh.uvs.resize(mesh.positions.len(), Vec2::ZERO);
    }
    mesh.compute_tangents();
    mesh.validate().map_err(|e| EngineError::Other(e.to_string()))?;
    Ok(mesh)
}

fn count_nodes(nodes: &[SppLayerNode]) -> usize {
    nodes.iter().map(|n| 1 + count_nodes(&n.children)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generator_and_filter_names_map() {
        assert!(matches!(generator_kind_from_name("mg_metal_edge_wear"), Some(GeneratorKind::EdgeWear)));
        assert!(matches!(generator_kind_from_name("mg_dirt"), Some(GeneratorKind::Dirt)));
        assert_eq!(filter_id_from_name("matfinish_hammered"), Some("matFinishHammered"));
        assert_eq!(filter_id_from_name("blur_hq"), Some("blur"));
        assert!(matches!(blend_mode("add"), BlendMode::LinearDodge));
    }

    fn env_file(key: &str) -> Option<std::path::PathBuf> {
        let path = std::env::var(key).ok()?;
        let path = std::path::PathBuf::from(path);
        path.is_file().then_some(path)
    }

    #[test]
    fn smart_material_imports_generators_from_env() {
        let Some(path) = env_file("TEXTURECRAFT_TEST_SPSM") else { return };
        let mut session = Session::new_project("ball", texturecraft_geom::primitives::material_ball(), "builtin:material_ball", None, 256).unwrap();
        dispatch(&mut session, path.to_str().unwrap(), Place::Selection).unwrap();
        let generators = session
            .doc
            .project
            .layers
            .values()
            .filter(|layer| layer.mask.as_ref().is_some_and(|mask| mask.effects.iter().any(|e| matches!(e.kind, EffectKind::Generator(_)))))
            .count();
        assert!(generators > 0, "the smart material should import at least one generator");
    }

    #[test]
    fn project_opens_from_env() {
        let Some(path) = env_file("TEXTURECRAFT_TEST_SPP") else { return };
        let session = open_project(path.to_str().unwrap()).unwrap();
        assert!(!session.mesh.positions.is_empty());
        assert!(session.doc.project.layers.len() > 1, "{}", session.log.last().unwrap_or(&String::new()));
    }
}
