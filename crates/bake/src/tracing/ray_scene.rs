use super::{Hemisphere, Query, Ray};
use crate::BakeError;
use texturecraft_geom::Hit;

pub trait RayScene: Send {
    fn closest(&mut self, mesh: usize, rays: &[Ray], ignore_backface: bool) -> Result<Vec<Option<Hit>>, BakeError>;
    fn hemispheres(&mut self, passes: &[Hemisphere], queries: &[Query]) -> Result<Vec<Vec<[f32; 4]>>, BakeError>;

    /// Rays one [`RayScene::closest`] call should receive. A call ends in a GPU readback, so a
    /// GPU scene raises this toward a full dispatch that still fits the device.
    fn ray_batch(&self) -> usize {
        1 << 20
    }

    /// Samples to trace between progress updates. One sample is one hemisphere query.
    fn sample_batch(&self) -> usize {
        1 << 18
    }
}
