//! In-process `.spp` (HDF5) reader.

mod container;
mod mesh_maps;
mod stage;
mod texture_cache;

pub use container::{SppContainer, read_container};
pub use mesh_maps::{CachedMeshMap, SceneMapKind, decode_scene_maps};
pub use texture_cache::{CachedMaterialChannels, decode_texture_cache};
