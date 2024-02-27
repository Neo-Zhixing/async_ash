use ash::extensions::ext;
use ash::{prelude::VkResult, vk};
use bevy_app::Plugin;
use std::ffi::CStr;

use std::sync::RwLock;

use crate::plugin::RhyoliteApp;

#[derive(Default)]
pub struct DebugUtilsPlugin;

impl Plugin for DebugUtilsPlugin {
    fn build(&self, app: &mut bevy_app::App) {
        app.add_instance_extension_named(ash::extensions::ext::DebugUtils::name())
            .unwrap();
        app.add_instance_meta(Box::new(|entry, instance| {
            Box::new(DebugUtilsMessenger::new(entry, instance))
        }));
    }
    fn finish(&self, _app: &mut bevy_app::App) {}
}

pub type DebugUtilsMessengerCallback = fn(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    types: vk::DebugUtilsMessageTypeFlagsEXT,
    callback_data: &DebugUtilsMessengerCallbackData,
);

pub struct DebugUtilsMessengerCallbackData<'a> {
    /// Identifies the particular message ID that is associated with the provided message.
    /// If the message corresponds to a validation layer message, then this string may contain
    /// the portion of the Vulkan specification that is believed to have been violated.
    pub message_id_name: &'a CStr,
    /// The ID number of the triggering message. If the message corresponds to a validation layer
    /// message, then this number is related to the internal number associated with the message
    /// being triggered.
    pub message_id_number: i32,
    /// Details on the trigger conditions
    pub message: &'a CStr,
    pub queue_labels: &'a [vk::DebugUtilsLabelEXT],
    pub cmd_buf_labels: &'a [vk::DebugUtilsLabelEXT],
    pub objects: &'a [vk::DebugUtilsObjectNameInfoEXT],
}

pub struct DebugUtilsMessenger {
    pub(crate) debug_utils: ext::DebugUtils,
    pub(crate) messenger: vk::DebugUtilsMessengerEXT,
    callbacks: RwLock<Vec<DebugUtilsMessengerCallback>>,
}
impl Drop for DebugUtilsMessenger {
    fn drop(&mut self) {
        unsafe {
            self.debug_utils
                .destroy_debug_utils_messenger(self.messenger, None);
        }
    }
}

impl DebugUtilsMessenger {
    pub fn new(entry: &ash::Entry, instance: &ash::Instance) -> VkResult<Box<Self>> {
        let debug_utils = ext::DebugUtils::new(entry, instance);

        let mut this = Box::new(Self {
            debug_utils,
            messenger: vk::DebugUtilsMessengerEXT::default(),
            callbacks: RwLock::new(vec![default_callback]),
        });
        let messenger = unsafe {
            let p_user_data = this.as_mut() as *mut Self as *mut std::ffi::c_void;
            // Safety:
            // The application must ensure that vkCreateDebugUtilsMessengerEXT is not executed in parallel
            // with any Vulkan command that is also called with instance or child of instance as the dispatchable argument.
            // We do this by taking a mutable reference to Instance.
            this.debug_utils.create_debug_utils_messenger(
                &vk::DebugUtilsMessengerCreateInfoEXT {
                    message_severity: vk::DebugUtilsMessageSeverityFlagsEXT::VERBOSE
                        | vk::DebugUtilsMessageSeverityFlagsEXT::INFO
                        | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                        | vk::DebugUtilsMessageSeverityFlagsEXT::ERROR,
                    message_type: vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                        | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                        | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
                    pfn_user_callback: Some(debug_utils_callback),
                    // This is self-referencing: Self contains `vk::DebugUtilsMessengerEXT` which then
                    // contains a pointer to Self. It's fine because Self was boxed.
                    p_user_data,
                    ..Default::default()
                },
                None,
            )?
        };
        this.messenger = messenger;
        Ok(this)
    }
    pub fn add_callback(&self, callback: DebugUtilsMessengerCallback) {
        let mut callbacks = self.callbacks.write().unwrap();
        callbacks.push(callback);
    }
}

unsafe extern "system" fn debug_utils_callback(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    types: vk::DebugUtilsMessageTypeFlagsEXT,
    callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    user_data: *mut std::ffi::c_void,
) -> vk::Bool32 {
    let this: &DebugUtilsMessenger =
        &*(user_data as *mut DebugUtilsMessenger as *const DebugUtilsMessenger);
    let callback_data_raw = &*callback_data;
    let callback_data = DebugUtilsMessengerCallbackData {
        message_id_number: callback_data_raw.message_id_number,
        message_id_name: CStr::from_ptr(callback_data_raw.p_message_id_name),
        message: CStr::from_ptr(callback_data_raw.p_message),
        queue_labels: std::slice::from_raw_parts(
            callback_data_raw.p_queue_labels,
            callback_data_raw.queue_label_count as usize,
        ),
        cmd_buf_labels: std::slice::from_raw_parts(
            callback_data_raw.p_cmd_buf_labels,
            callback_data_raw.cmd_buf_label_count as usize,
        ),
        objects: std::slice::from_raw_parts(
            callback_data_raw.p_objects,
            callback_data_raw.object_count as usize,
        ),
    };
    for callback in this.callbacks.read().unwrap().iter() {
        (callback)(severity, types, &callback_data)
    }
    // The callback returns a VkBool32, which is interpreted in a layer-specified manner.
    // The application should always return VK_FALSE. The VK_TRUE value is reserved for use in layer development.
    vk::FALSE
}

fn default_callback(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _types: vk::DebugUtilsMessageTypeFlagsEXT,
    callback_data: &DebugUtilsMessengerCallbackData,
) {
    use tracing::Level;
    let level = match severity {
        vk::DebugUtilsMessageSeverityFlagsEXT::VERBOSE => Level::DEBUG,
        vk::DebugUtilsMessageSeverityFlagsEXT::INFO => Level::INFO,
        vk::DebugUtilsMessageSeverityFlagsEXT::WARNING => Level::WARN,
        vk::DebugUtilsMessageSeverityFlagsEXT::ERROR => Level::ERROR,
        _ => Level::TRACE,
    };

    if level == Level::ERROR {
        let bt = std::backtrace::Backtrace::capture();
        if bt.status() == std::backtrace::BacktraceStatus::Captured {
            println!("{}", bt);
        }
    }

    match level {
        Level::ERROR => {
            tracing::error!(message=?callback_data.message_id_name, id=callback_data.message_id_number, detail=?callback_data.message)
        }
        Level::WARN => {
            tracing::warn!(message=?callback_data.message_id_name, id=callback_data.message_id_number, detail=?callback_data.message)
        }
        Level::DEBUG => {
            tracing::debug!(message=?callback_data.message_id_name, id=callback_data.message_id_number, detail=?callback_data.message)
        }
        Level::TRACE => {
            tracing::trace!(message=?callback_data.message_id_name, id=callback_data.message_id_number, detail=?callback_data.message)
        }
        Level::INFO => {
            tracing::info!(message=?callback_data.message_id_name, id=callback_data.message_id_number, detail=?callback_data.message)
        }
    };
}

/*
/// Vulkan Object that can be associated with a name and/or a tag.
pub trait DebugObject: crate::HasDevice {
    fn object_handle(&mut self) -> u64;
    const OBJECT_TYPE: vk::ObjectType;
    fn set_name_cstr(&mut self, cstr: &CStr) -> VkResult<()> {
        unsafe {
            let raw_device = self.device().handle();
            let object_handle = self.object_handle();
            self.device()
                .instance()
                .debug_utils()
                .debug_utils
                .set_debug_utils_object_name(
                    raw_device,
                    &vk::DebugUtilsObjectNameInfoEXT {
                        object_type: Self::OBJECT_TYPE,
                        object_handle,
                        p_object_name: cstr.as_ptr(),
                        ..Default::default()
                    },
                )?;
        }
        Ok(())
    }
    fn set_name(&mut self, name: &str) -> VkResult<()> {
        let cstr = CString::new(name).expect("Name cannot contain null bytes");
        self.set_name_cstr(cstr.as_c_str())?;
        Ok(())
    }
    fn with_name(mut self, name: &str) -> VkResult<Self>
    where
        Self: Sized,
    {
        self.set_name(name)?;
        Ok(self)
    }
    fn remove_name(&mut self) {
        unsafe {
            let raw_device = self.device().handle();
            let object_handle = self.object_handle();
            self.device()
                .instance()
                .debug_utils()
                .debug_utils
                .set_debug_utils_object_name(
                    raw_device,
                    &vk::DebugUtilsObjectNameInfoEXT {
                        object_type: Self::OBJECT_TYPE,
                        object_handle,
                        p_object_name: std::ptr::null(),
                        ..Default::default()
                    },
                )
                .unwrap();
        }
    }
}
*/
