use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

use bevy_ecs::system::Resource;

#[derive(Resource, Clone)]
pub struct Allocator(rhyolite::Allocator);
impl Allocator {
    pub fn new(inner: rhyolite::Allocator) -> Self {
        Allocator(inner)
    }
    pub fn into_inner(self) -> rhyolite::Allocator {
        self.0
    }
}
impl Deref for Allocator {
    type Target = rhyolite::Allocator;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
#[derive(Resource, Clone)]
pub struct Device(Arc<rhyolite::Device>);

impl Device {
    pub fn new(inner: Arc<rhyolite::Device>) -> Self {
        Device(inner)
    }
    pub fn inner(&self) -> &Arc<rhyolite::Device> {
        &self.0
    }
}

impl Deref for Device {
    type Target = Arc<rhyolite::Device>;

    fn deref(&self) -> &Self::Target {
        self.inner()
    }
}

pub enum SharingMode {
    Exclusive,
    Concurrent { queue_family_indices: Vec<u32> },
}

impl Default for SharingMode {
    fn default() -> Self {
        Self::Exclusive
    }
}

impl<'a> From<&'a SharingMode> for rhyolite::SharingMode<'a> {
    fn from(value: &'a SharingMode) -> Self {
        match value {
            SharingMode::Exclusive => rhyolite::SharingMode::Exclusive,
            SharingMode::Concurrent {
                queue_family_indices,
            } => rhyolite::SharingMode::Concurrent {
                queue_family_indices: &queue_family_indices,
            },
        }
    }
}

#[derive(Resource, Clone)]
pub struct PipelineCache(Arc<rhyolite::PipelineCache>);
impl PipelineCache {
    pub fn inner(&self) -> &Arc<rhyolite::PipelineCache> {
        &self.0
    }
}