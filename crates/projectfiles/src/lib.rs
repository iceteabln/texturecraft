//! Project files TextureCraft can open.
//!
//! `.spp` projects, `.spsm` smart materials and `.spmsk` smart masks are HDF5 containers with a
//! layer document. The layer stack (fills, folders, mask generators, procedural noises, levels and
//! the filters the document stores as levels or procedural fills) is decoded in process.
//! `.sbsar` generators map onto TextureCraft's mesh-map generators. Known filters map onto the
//! filter library. Compiled material graphs are not executed. Mesh maps are baked here; the
//! file's baked images are not read.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![allow(
    clippy::absurd_extreme_comparisons,
    clippy::chunks_exact_to_as_chunks,
    clippy::collapsible_if,
    clippy::if_same_then_else,
    clippy::manual_checked_ops,
    clippy::manual_clamp,
    clippy::manual_is_multiple_of,
    clippy::manual_range_contains,
    clippy::redundant_closure,
    clippy::too_many_arguments,
    clippy::type_complexity
)]

pub mod assbin;
pub mod hbo;
mod native;
mod pod;
mod stack;

pub use assbin::{SceneMesh, import_assbin};
pub use hbo::{build_stack_from_document, decode_spmsk, decode_spsm};
pub use native::{SppContainer, read_container};
pub use stack::{
    SppChannelBlend, SppContentEffect, SppContentFilter, SppLayerNode, SppMaskEffect, SppMaskEffectKind, SppMaterialStack, SppProceduralChannel, SppSbsarRef,
    SppStackFile, SppTextureSetStack, filter_from_graph_url,
};
