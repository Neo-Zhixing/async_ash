use ash::vk;
use bevy::asset::{AssetId, Assets};

use crate::{
    deferred::{DeferredOperationTaskPool, Task},
    dispose::RenderObject,
    shader::ShaderModule,
    Device, HasDevice,
};

mod cache;
mod compute;
mod graphics;
mod layout;

pub use cache::*;
pub use compute::*;
pub use graphics::*;
pub use layout::*;

pub trait Pipeline: Sized + Send + Sync + 'static {
    type BuildInfo: PipelineBuildInfo;
    const TYPE: vk::PipelineBindPoint;
    fn from_built(
        info: &mut Self::BuildInfo,
        item: <Self::BuildInfo as PipelineBuildInfo>::Pipeline,
    ) -> Self;

    /// Same as `from_built`, but takes ownership of the build info.
    /// This function is called instead when shader hot reloading is disabled.
    /// Because the pipeline will never be built again, the implementation may take ownership of the build info.
    fn from_built_with_owned_info(
        mut info: Self::BuildInfo,
        item: <Self::BuildInfo as PipelineBuildInfo>::Pipeline,
    ) -> Self {
        Self::from_built(&mut info, item)
    }

    fn as_raw(&self) -> vk::Pipeline;
}

impl<T: Pipeline> Pipeline for RenderObject<T> {
    type BuildInfo = T::BuildInfo;
    const TYPE: vk::PipelineBindPoint = T::TYPE;
    fn as_raw(&self) -> vk::Pipeline {
        T::as_raw(self.get())
    }
    fn from_built(
        info: &mut Self::BuildInfo,
        item: <Self::BuildInfo as PipelineBuildInfo>::Pipeline,
    ) -> Self {
        RenderObject::new(T::from_built(info, item))
    }
}

pub trait PipelineBuildInfo {
    type Pipeline;
    fn build(
        &mut self,
        pool: &DeferredOperationTaskPool,
        assets: &Assets<ShaderModule>,
        cache: vk::PipelineCache,
    ) -> Option<Task<Self::Pipeline>>;

    /// List of all shaders used by this pipeline.
    /// Only called when shader hot reloading is enabled.
    fn all_shaders(&self) -> impl Iterator<Item = AssetId<ShaderModule>>;
}

pub struct PipelineInner {
    device: Device,
    pipeline: vk::Pipeline,
}
impl HasDevice for PipelineInner {
    fn device(&self) -> &Device {
        &self.device
    }
}
impl PipelineInner {
    pub fn from_raw(device: Device, raw: vk::Pipeline) -> Self {
        Self {
            device,
            pipeline: raw,
        }
    }
    pub fn raw(&self) -> vk::Pipeline {
        self.pipeline
    }
}
impl Drop for PipelineInner {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_pipeline(self.pipeline, None);
        }
    }
}
