use ash::vk::{ExtensionMeta, PromotionStatus};
use ash::{khr, vk};
use bevy::ecs::prelude::*;
use bevy::utils::HashSet;
use bevy::{app::prelude::*, asset::AssetApp, utils::hashbrown::HashMap};
use std::{
    any::Any,
    collections::BTreeMap,
    ffi::{c_char, CStr, CString},
    ops::Deref,
    sync::Arc,
};

use crate::extensions::{Extension, ExtensionNotFoundError};
use crate::{Device, Feature, Instance, PhysicalDevice, PhysicalDeviceFeaturesSetup, Version};
use cstr::cstr;

#[derive(Clone)]
pub struct LayerProperties {
    pub spec_version: Version,
    pub implementation_version: Version,
    pub description: String,
}

/// This is the point where the Vulkan instance and device are created.
/// All instance plugins must be added before RhyolitePlugin.
/// All device plugins must be added after RhyolitePlugin.
pub struct RhyolitePlugin {
    pub application_name: CString,
    pub application_version: Version,
    pub engine_name: CString,
    pub engine_version: Version,
    pub api_version: Version,

    pub physical_device_index: usize,
}
unsafe impl Send for RhyolitePlugin {}
unsafe impl Sync for RhyolitePlugin {}
impl Default for RhyolitePlugin {
    fn default() -> Self {
        Self {
            application_name: cstr!(b"Unnamed Application").to_owned(),
            application_version: Default::default(),
            engine_name: cstr!(b"Unnamed Engine").to_owned(),
            engine_version: Default::default(),
            api_version: Version::new(0, 1, 2, 0),
            physical_device_index: 0,
        }
    }
}
#[derive(Resource, Clone)]
pub struct VulkanEntry(Arc<ash::Entry>);
impl Deref for VulkanEntry {
    type Target = ash::Entry;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Default for VulkanEntry {
    fn default() -> Self {
        Self(Arc::new(unsafe { ash::Entry::load().unwrap() }))
    }
}
pub(crate) type InstanceMetaBuilder =
    Box<dyn FnOnce(&ash::Entry, &ash::Instance) -> Box<dyn Any + Send + Sync> + Send + Sync>;
pub(crate) type DeviceMetaBuilder =
    Box<dyn FnOnce(&ash::Instance, &mut ash::Device) -> Box<dyn Any + Send + Sync> + Send + Sync>;
#[derive(Resource, Default)]
struct DeviceExtensions {
    available_extensions: BTreeMap<CString, Version>,
    enabled_extensions: HashSet<&'static CStr>,
    extension_builders: HashMap<&'static CStr, Option<DeviceMetaBuilder>>,
}
impl DeviceExtensions {
    fn set_pdevice(&mut self, pdevice: &PhysicalDevice) {
        let extension_names = unsafe {
            pdevice
                .instance()
                .enumerate_device_extension_properties(pdevice.raw())
                .unwrap()
        };
        let extension_names = extension_names
            .into_iter()
            .map(|ext| {
                let str = ext.extension_name_as_c_str().unwrap();
                (str.to_owned(), Version(ext.spec_version))
            })
            .collect::<BTreeMap<CString, Version>>();
        self.available_extensions = extension_names;
    }
}
unsafe impl Send for DeviceExtensions {}
unsafe impl Sync for DeviceExtensions {}

#[derive(Resource)]
struct InstanceExtensions {
    available_extensions: BTreeMap<CString, Version>,
    enabled_extensions: HashMap<&'static CStr, Option<InstanceMetaBuilder>>,
}
impl FromWorld for InstanceExtensions {
    fn from_world(world: &mut World) -> Self {
        if world.contains_resource::<Instance>() {
            panic!("Instance extensions may only be added before the instance was created");
        }
        let entry = world.get_resource_or_insert_with::<VulkanEntry>(VulkanEntry::default);
        let available_extensions = unsafe { entry.enumerate_instance_extension_properties(None) }
            .unwrap()
            .into_iter()
            .map(|ext| {
                let str = ext.extension_name_as_c_str().unwrap();
                (str.to_owned(), Version(ext.spec_version))
            })
            .collect::<BTreeMap<CString, Version>>();
        Self {
            available_extensions,
            enabled_extensions: HashMap::new(),
        }
    }
}
unsafe impl Send for InstanceExtensions {}
unsafe impl Sync for InstanceExtensions {}

#[derive(Resource)]
struct InstanceLayers {
    available_layers: BTreeMap<CString, LayerProperties>,
    enabled_layers: Vec<*const c_char>,
}
impl FromWorld for InstanceLayers {
    fn from_world(world: &mut World) -> Self {
        if world.contains_resource::<Instance>() {
            panic!("Instance layers may only be added before the instance was created");
        }
        let entry = world.get_resource_or_insert_with::<VulkanEntry>(VulkanEntry::default);
        let available_layers = unsafe { entry.enumerate_instance_layer_properties() }
            .unwrap()
            .into_iter()
            .map(|layer| {
                let str = layer.layer_name_as_c_str().unwrap();
                (
                    str.to_owned(),
                    LayerProperties {
                        implementation_version: Version(layer.implementation_version),
                        spec_version: Version(layer.spec_version),
                        description: layer
                            .description_as_c_str()
                            .unwrap()
                            .to_str()
                            .unwrap()
                            .to_string(),
                    },
                )
            })
            .collect::<BTreeMap<CString, LayerProperties>>();
        Self {
            available_layers,
            enabled_layers: Vec::new(),
        }
    }
}
unsafe impl Send for InstanceLayers {}
unsafe impl Sync for InstanceLayers {}

impl Plugin for RhyolitePlugin {
    fn build(&self, app: &mut App) {
        #[allow(unused_mut)]
        let mut instance_create_flags = vk::InstanceCreateFlags::empty();
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            instance_create_flags |= vk::InstanceCreateFlags::ENUMERATE_PORTABILITY_KHR;
            app.add_instance_extension_named(ash::khr::portability_enumeration::NAME)
                .unwrap();

            app.add_instance_layer(cstr::cstr!(b"VK_LAYER_KHRONOS_validation"))
                .unwrap();
        }
        let mut instance_extensions = app.world_mut().remove_resource::<InstanceExtensions>();
        let instance_layers = app.world_mut().remove_resource::<InstanceLayers>();
        let entry: &VulkanEntry = &app
            .world_mut()
            .get_resource_or_insert_with(VulkanEntry::default);
        let enabled_extensions = instance_extensions
            .as_mut()
            .map(|a| std::mem::take(&mut a.enabled_extensions))
            .unwrap_or_default();
        let instance = Instance::create(
            entry.0.clone(),
            crate::InstanceCreateInfo {
                flags: instance_create_flags,
                enabled_extensions,
                enabled_layer_names: instance_layers
                    .as_ref()
                    .map(|f| f.enabled_layers.as_slice())
                    .unwrap_or(&[]),
                api_version: self.api_version,
                engine_name: self.engine_name.as_c_str(),
                engine_version: self.engine_version,
                application_name: self.application_name.as_c_str(),
                application_version: self.application_version,
            },
        )
        .unwrap();
        let physical_device = instance
            .enumerate_physical_devices()
            .unwrap()
            .skip(self.physical_device_index)
            .next()
            .unwrap();
        tracing::info!(
            "Using {:?} {:?} with memory model {:?}",
            physical_device.properties().device_type,
            physical_device.properties().device_name(),
            physical_device.properties().memory_model
        );
        let features = PhysicalDeviceFeaturesSetup::new(physical_device.clone());
        let mut device_extensions = app
            .world_mut()
            .get_resource_or_insert_with(DeviceExtensions::default);
        device_extensions.set_pdevice(&physical_device);

        app.insert_resource(instance)
            .insert_resource(physical_device)
            .insert_resource(features)
            .init_asset::<crate::shader::ShaderModule>()
            .init_asset::<crate::shader::loader::SpirvShaderSource>();
        // Add build pass
        app.get_schedule_mut(PostUpdate)
            .as_mut()
            .unwrap()
            .add_build_pass(rhyolite::ecs::RenderSystemsPass::new());

        // Required features
        app.enable_feature::<vk::PhysicalDeviceTimelineSemaphoreFeatures>(|f| {
            &mut f.timeline_semaphore
        })
        .unwrap();
        app.add_device_extension::<khr::synchronization2::Meta>()
            .unwrap();
        //app.add_device_extension::<khr::maintenance4::Meta>()
        //    .unwrap();
        app.enable_feature::<vk::PhysicalDeviceSynchronization2Features>(|f| {
            &mut f.synchronization2
        })
        .unwrap();

        // Optional extensions
        app.add_device_extension::<khr::deferred_host_operations::Meta>()
            .ok();

        // IF supported, must be enabled.
        app.add_device_extension_named(vk::KHR_PORTABILITY_SUBSET_NAME)
            .ok();

        #[cfg(feature = "glsl")]
        app.add_plugins(crate::shader::loader::GlslPlugin {
            target_vk_version: self.api_version,
        });

        app.add_plugins(crate::buffer::staging::StagingBeltPlugin);
        app.add_plugins(crate::pipeline::PipelineCachePlugin::default());

        app.register_type::<bevy::image::Image>()
            .init_asset::<bevy::image::Image>()
            .register_asset_reflect::<bevy::image::Image>();
    }
    fn finish(&self, app: &mut App) {
        let extension_settings: DeviceExtensions = app
            .world_mut()
            .remove_resource::<DeviceExtensions>()
            .unwrap();
        let features = app
            .world_mut()
            .remove_resource::<PhysicalDeviceFeaturesSetup>()
            .unwrap()
            .finalize();
        let physical_device: PhysicalDevice = app.world().resource::<PhysicalDevice>().clone();
        Device::create_in_world(
            app.world_mut(),
            physical_device.clone(),
            features,
            extension_settings.enabled_extensions,
            extension_settings.extension_builders,
        )
        .unwrap();

        // Add allocator
        app.world_mut().init_resource::<crate::Allocator>();
        //app.world_mut()
        //    .init_resource::<crate::task::AsyncTaskPool>();
        app.world_mut()
            .init_resource::<crate::DeferredOperationTaskPool>();
        app.init_asset_loader::<crate::shader::loader::SpirvLoader>();
    }
}

pub trait RhyoliteApp {
    /// Called in the [Plugin::build] phase of device plugins.
    /// Device plugins must be added after [RhyolitePlugin].
    fn add_device_extension<T: ExtensionMeta>(&mut self) -> Result<(), ExtensionNotFoundError>
    where
        T::Device: Send + Sync + 'static;

    /// Called in the [Plugin::build] phase of device plugins.
    /// Instance plugins must be added before [RhyolitePlugin].
    fn add_instance_extension<T: ExtensionMeta>(&mut self) -> Result<(), ExtensionNotFoundError>
    where
        T::Instance: Send + Sync + 'static,
        T::Device: Send + Sync + 'static;

    /// Called in the [Plugin::build] phase of device plugins.
    /// Device plugins must be added after [RhyolitePlugin].
    fn add_device_extension_named(
        &mut self,
        extension: &'static CStr,
    ) -> Result<(), ExtensionNotFoundError>;

    /// Called in the [Plugin::build] phase of instance plugins.
    /// Instance plugins must be added before [RhyolitePlugin].
    fn add_instance_extension_named(
        &mut self,
        extension: &'static CStr,
    ) -> Result<(), ExtensionNotFoundError>;

    /// Called in the [Plugin::build] phase of instance plugins.
    /// Instance plugins must be added after [RhyolitePlugin].
    fn add_instance_layer(&mut self, layer: &'static CStr) -> Option<LayerProperties>;

    /// Called in the [Plugin::build] phase of device plugins.
    /// Device plugins must be added after [RhyolitePlugin].
    fn enable_feature<T: Feature + Default + 'static>(
        &mut self,
        selector: impl FnMut(&mut T) -> &mut vk::Bool32,
    ) -> FeatureEnableResult;
}

impl RhyoliteApp for App {
    fn add_device_extension<T: Extension>(&mut self) -> Result<(), ExtensionNotFoundError>
    where
        T::Device: Send + Sync + 'static,
    {
        if let PromotionStatus::PromotedToCore(promoted_extension) = T::PROMOTION_STATUS {
            let promoted_extension = Version(promoted_extension);
            let instance = self.world().resource::<Instance>();
            if instance.api_version() >= promoted_extension {
                return Ok(());
            }
        }
        let Some(mut extension_settings) = self.world_mut().get_resource_mut::<DeviceExtensions>()
        else {
            panic!("Device extensions may only be added after the instance was created. Add RhyolitePlugin before all device plugins.")
        };
        if let Some(_v) = extension_settings.available_extensions.get(T::NAME) {
            extension_settings.enabled_extensions.insert(T::NAME);
            extension_settings.extension_builders.insert(
                T::NAME,
                Some(Box::new(|instance, device| {
                    let ext = T::new_device(instance, device);
                    T::promote_device(device, &ext);
                    Box::new(ext)
                })),
            );
            Ok(())
        } else {
            Err(ExtensionNotFoundError)
        }
    }

    fn add_instance_extension<T: Extension>(&mut self) -> Result<(), ExtensionNotFoundError>
    where
        T::Instance: Send + Sync + 'static,
        T::Device: Send + Sync + 'static,
    {
        if self.world().get_resource::<Instance>().is_some() {
            panic!("Instance extensions may only be added before the instance was created. Add RhyolitePlugin after all instance plugins.")
        }
        if let PromotionStatus::PromotedToCore(promoted_extension) = T::PROMOTION_STATUS {
            let promoted_extension = Version(promoted_extension);
            let instance = self.world().resource::<Instance>();
            if instance.api_version() >= promoted_extension {
                return Ok(());
            }
        }
        let mut instance_extensions = if let Some(extension_settings) =
            self.world_mut().get_resource_mut::<InstanceExtensions>()
        {
            extension_settings
        } else {
            let extension_settings = InstanceExtensions::from_world(self.world_mut());
            self.world_mut().insert_resource(extension_settings);
            self.world_mut().resource_mut::<InstanceExtensions>()
        };
        if let Some(_v) = instance_extensions.available_extensions.get(T::NAME) {
            instance_extensions.enabled_extensions.insert(
                T::NAME,
                Some(Box::new(|entry, instance| {
                    let ext = T::new_instance(entry, instance);
                    Box::new(ext)
                })),
            );
            if std::any::TypeId::of::<T::Device>() != std::any::TypeId::of::<()>() {
                let mut device_extensions = self
                    .world_mut()
                    .get_resource_or_insert_with(DeviceExtensions::default);
                device_extensions.extension_builders.insert(
                    T::NAME,
                    Some(Box::new(|instance, device| {
                        let ext = T::new_device(instance, device);
                        Box::new(ext)
                    })),
                );
            }

            Ok(())
        } else {
            Err(ExtensionNotFoundError)
        }
    }

    fn add_device_extension_named(
        &mut self,
        extension: &'static CStr,
    ) -> Result<(), ExtensionNotFoundError> {
        let Some(mut extension_settings) = self.world_mut().get_resource_mut::<DeviceExtensions>()
        else {
            panic!("Device extensions may only be added after the instance was created. Add RhyolitePlugin before all device plugins.")
        };
        if let Some(_v) = extension_settings.available_extensions.get(extension) {
            extension_settings.enabled_extensions.insert(extension);
            extension_settings
                .extension_builders
                .insert(extension, None);
            Ok(())
        } else {
            Err(ExtensionNotFoundError)
        }
    }
    fn add_instance_extension_named(
        &mut self,
        extension: &'static CStr,
    ) -> Result<(), ExtensionNotFoundError> {
        let extension_settings = self.world_mut().get_resource_mut::<InstanceExtensions>();
        let mut extension_settings = match extension_settings {
            Some(extension_settings) => extension_settings,
            None => {
                let extension_settings = InstanceExtensions::from_world(self.world_mut());
                self.world_mut().insert_resource(extension_settings);
                self.world_mut().resource_mut::<InstanceExtensions>()
            }
        };
        if let Some(_v) = extension_settings.available_extensions.get(extension) {
            extension_settings
                .enabled_extensions
                .insert(extension, None);
            Ok(())
        } else {
            Err(ExtensionNotFoundError)
        }
    }
    fn add_instance_layer(&mut self, layer: &'static CStr) -> Option<LayerProperties> {
        let layers = self.world_mut().get_resource_mut::<InstanceLayers>();
        let mut layers = match layers {
            Some(layers) => layers,
            None => {
                let extension_settings = InstanceLayers::from_world(self.world_mut());
                self.world_mut().insert_resource(extension_settings);
                self.world_mut().resource_mut::<InstanceLayers>()
            }
        };
        if let Some(v) = layers.available_layers.get(layer) {
            let v = v.clone();
            layers.enabled_layers.push(layer.as_ptr());

            let vulkan_entry = self.world_mut().resource::<VulkanEntry>();
            let additional_instance_extensions = unsafe {
                vulkan_entry
                    .enumerate_instance_extension_properties(Some(layer))
                    .unwrap()
            };

            let instance_extensions = self.world_mut().get_resource_mut::<InstanceExtensions>();
            let mut instance_extensions = match instance_extensions {
                Some(instance_extensions) => instance_extensions,
                None => {
                    let instance_extensions = InstanceExtensions::from_world(self.world_mut());
                    self.world_mut().insert_resource(instance_extensions);
                    self.world_mut().resource_mut::<InstanceExtensions>()
                }
            };
            instance_extensions.available_extensions.extend(
                additional_instance_extensions.into_iter().map(|a| {
                    (
                        a.extension_name_as_c_str().unwrap().to_owned(),
                        Version(a.spec_version),
                    )
                }),
            );

            Some(v)
        } else {
            None
        }
    }
    fn enable_feature<'a, T: Feature + Default + 'static>(
        &'a mut self,
        selector: impl FnMut(&mut T) -> &mut vk::Bool32,
    ) -> FeatureEnableResult<'a> {
        let device_extension = self.world().resource::<DeviceExtensions>();
        let instance = self.world().resource::<Instance>();
        if !device_extension
            .extension_builders
            .contains_key(T::REQUIRED_DEVICE_EXT)
        {
            if let PromotionStatus::PromotedToCore(promoted_version) = T::PROMOTION_STATUS {
                if instance.api_version() < Version(promoted_version) {
                    tracing::warn!(
                        "Feature {:?} requires either Vulkan {} or enabling extension {:?}. Current Vulkan version: {}",
                        std::any::type_name::<T>(),
                        promoted_version,
                        T::REQUIRED_DEVICE_EXT,
                        instance.api_version()
                    );
                }
            } else {
                tracing::warn!(
                    "Feature {:?} requires enabling extension {:?}",
                    std::any::type_name::<T>(),
                    T::REQUIRED_DEVICE_EXT
                );
            }
        }
        let mut features = self
            .world_mut()
            .resource_mut::<PhysicalDeviceFeaturesSetup>();
        if features.enable_feature::<T>(selector).is_none() {
            return FeatureEnableResult::NotFound { app: self };
        }
        FeatureEnableResult::Success
    }
}

pub enum FeatureEnableResult<'a> {
    Success,
    NotFound { app: &'a mut App },
}
impl<'a> FeatureEnableResult<'a> {
    pub fn exists(&self) -> bool {
        match self {
            FeatureEnableResult::Success => true,
            FeatureEnableResult::NotFound { .. } => false,
        }
    }
    #[track_caller]
    pub fn unwrap(&self) {
        match self {
            FeatureEnableResult::Success => (),
            FeatureEnableResult::NotFound { .. } => {
                panic!("Feature not found")
            }
        }
    }
}
