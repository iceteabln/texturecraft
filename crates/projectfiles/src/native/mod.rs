//! In-process `.spp` (HDF5) reader.

mod container;
mod stage;

pub use container::{SppContainer, read_container};
