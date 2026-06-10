use std::ffi::CString;

use kuria_plugin::{CHookArgs, CHookResult, PLUGIN_ABI_VERSION, PluginVTable};
use serde::Serialize;
use tracing::{error, info};

use crate::config::Config;

#[derive(Debug, Clone, Serialize)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginLoadError {
    pub path: String,
    pub error: String,
}

/// A loaded plugin instance: holds the library handle (prevents unload) and vtable.
struct PluginInstance {
    info: PluginInfo,
    _library: libloading::Library,
    vtable: *const PluginVTable,
}

// Safety: The vtable pointers are valid for the lifetime of the Library handle.
// All FFI calls go through the vtable which is created by the plugin.
unsafe impl Send for PluginInstance {}
unsafe impl Sync for PluginInstance {}

/// Manages loaded plugins. Shared across SMTP and Web servers via Arc.
pub struct PluginManager {
    plugins: Vec<PluginInstance>,
    load_errors: Vec<PluginLoadError>,
}

impl PluginManager {
    /// Load all plugins specified in the config.
    pub fn load(config: &Config) -> anyhow::Result<Self> {
        let mut plugins = Vec::new();
        let mut load_errors = Vec::new();

        let plugins_config = match &config.plugins {
            Some(pc) if pc.enabled => pc,
            _ => {
                info!("Plugin system disabled");
                return Ok(Self {
                    plugins,
                    load_errors,
                });
            }
        };

        for path in &plugins_config.paths {
            match Self::load_single_plugin(path) {
                Ok(instance) => {
                    info!("Loaded plugin: {} from {}", instance.info.name, path);
                    plugins.push(instance);
                }
                Err(e) => {
                    error!("Failed to load plugin {}: {}", path, e);
                    load_errors.push(PluginLoadError {
                        path: path.clone(),
                        error: e.to_string(),
                    });
                }
            }
        }

        info!("{} plugin(s) loaded", plugins.len());
        Ok(Self {
            plugins,
            load_errors,
        })
    }

    fn load_single_plugin(path: &str) -> anyhow::Result<PluginInstance> {
        unsafe {
            let library = libloading::Library::new(path)
                .map_err(|e| anyhow::anyhow!("Failed to load library: {}", e))?;

            let create_fn: libloading::Symbol<unsafe extern "C" fn() -> *const PluginVTable> =
                library
                    .get(b"kuria_plugin_create")
                    .map_err(|e| anyhow::anyhow!("Missing kuria_plugin_create: {}", e))?;

            let vtable = create_fn();
            if vtable.is_null() {
                return Err(anyhow::anyhow!("kuria_plugin_create returned null"));
            }

            let vtable_ref = &*vtable;

            // Check ABI version
            if vtable_ref.version != PLUGIN_ABI_VERSION {
                return Err(anyhow::anyhow!(
                    "Plugin ABI version mismatch: got {}, expected {}",
                    vtable_ref.version,
                    PLUGIN_ABI_VERSION
                ));
            }

            // Read metadata
            let (name, version, description) = if !vtable_ref.metadata.is_null() {
                (
                    read_cstr((*vtable_ref.metadata).name).unwrap_or_else(|| "unknown".to_string()),
                    read_cstr((*vtable_ref.metadata).version).unwrap_or_default(),
                    read_cstr((*vtable_ref.metadata).description).unwrap_or_default(),
                )
            } else {
                ("unknown".to_string(), String::new(), String::new())
            };

            Ok(PluginInstance {
                info: PluginInfo {
                    name,
                    version,
                    description,
                    path: path.to_string(),
                },
                _library: library,
                vtable,
            })
        }
    }

    pub fn plugins_info(&self) -> Vec<PluginInfo> {
        self.plugins
            .iter()
            .map(|plugin| plugin.info.clone())
            .collect()
    }

    pub fn load_errors(&self) -> &[PluginLoadError] {
        &self.load_errors
    }

    /// Call `on_init` for all loaded plugins.
    pub fn call_init(&self, config: &Config) {
        let config_json = serde_json::to_string(config).unwrap_or_default();
        let config_cstr = CString::new(config_json).unwrap_or_default();

        for plugin in &self.plugins {
            unsafe {
                let args = CHookArgs {
                    peer_addr: std::ptr::null(),
                    is_tls: false,
                    sender: std::ptr::null(),
                    recipient: std::ptr::null(),
                    recipients: std::ptr::null(),
                    recipients_len: 0,
                    raw_message: std::ptr::null(),
                    raw_message_len: 0,
                    config_json: config_cstr.as_ptr(),
                    method: std::ptr::null(),
                    path: std::ptr::null(),
                    headers_json: std::ptr::null(),
                    query: std::ptr::null(),
                };

                let result = ((*plugin.vtable).init)(&args);
                if !result.is_null() {
                    ((*plugin.vtable).free_result)(result);
                }
            }
            info!("Plugin {} initialized", plugin.info.name);
        }
    }

    /// Call `on_shutdown` for all loaded plugins.
    pub fn call_shutdown(&self) {
        for plugin in &self.plugins {
            unsafe {
                let result = ((*plugin.vtable).shutdown)();
                if !result.is_null() {
                    ((*plugin.vtable).free_result)(result);
                }
            }
            info!("Plugin {} shut down", plugin.info.name);
        }
    }

    /// Call `on_smtp_connect` for all plugins. Returns the first rejection, if any.
    pub fn call_smtp_connect(&self, peer_addr: &str, is_tls: bool) -> Option<HookResult> {
        let peer_cstr = CString::new(peer_addr).unwrap_or_default();

        for plugin in &self.plugins {
            unsafe {
                let args = CHookArgs {
                    peer_addr: peer_cstr.as_ptr(),
                    is_tls,
                    sender: std::ptr::null(),
                    recipient: std::ptr::null(),
                    recipients: std::ptr::null(),
                    recipients_len: 0,
                    raw_message: std::ptr::null(),
                    raw_message_len: 0,
                    config_json: std::ptr::null(),
                    method: std::ptr::null(),
                    path: std::ptr::null(),
                    headers_json: std::ptr::null(),
                    query: std::ptr::null(),
                };

                if let Some(result) = self.invoke_hook(plugin, kuria_plugin::ON_SMTP_CONNECT, &args)
                    && result.reject
                {
                    return Some(result);
                }
            }
        }
        None
    }

    /// Call `on_smtp_from` for all plugins. Returns the first rejection, if any.
    pub fn call_smtp_from(
        &self,
        sender: &str,
        peer_addr: &str,
        is_tls: bool,
    ) -> Option<HookResult> {
        let sender_cstr = CString::new(sender).unwrap_or_default();
        let peer_cstr = CString::new(peer_addr).unwrap_or_default();

        for plugin in &self.plugins {
            unsafe {
                let args = CHookArgs {
                    peer_addr: peer_cstr.as_ptr(),
                    is_tls,
                    sender: sender_cstr.as_ptr(),
                    recipient: std::ptr::null(),
                    recipients: std::ptr::null(),
                    recipients_len: 0,
                    raw_message: std::ptr::null(),
                    raw_message_len: 0,
                    config_json: std::ptr::null(),
                    method: std::ptr::null(),
                    path: std::ptr::null(),
                    headers_json: std::ptr::null(),
                    query: std::ptr::null(),
                };

                if let Some(result) = self.invoke_hook(plugin, kuria_plugin::ON_SMTP_FROM, &args)
                    && result.reject
                {
                    return Some(result);
                }
            }
        }
        None
    }

    /// Call `on_smtp_to` for all plugins. Returns the first rejection, if any.
    pub fn call_smtp_to(
        &self,
        recipient: &str,
        sender: &str,
        peer_addr: &str,
        is_tls: bool,
    ) -> Option<HookResult> {
        let rcpt_cstr = CString::new(recipient).unwrap_or_default();
        let sender_cstr = CString::new(sender).unwrap_or_default();
        let peer_cstr = CString::new(peer_addr).unwrap_or_default();

        for plugin in &self.plugins {
            unsafe {
                let args = CHookArgs {
                    peer_addr: peer_cstr.as_ptr(),
                    is_tls,
                    sender: sender_cstr.as_ptr(),
                    recipient: rcpt_cstr.as_ptr(),
                    recipients: std::ptr::null(),
                    recipients_len: 0,
                    raw_message: std::ptr::null(),
                    raw_message_len: 0,
                    config_json: std::ptr::null(),
                    method: std::ptr::null(),
                    path: std::ptr::null(),
                    headers_json: std::ptr::null(),
                    query: std::ptr::null(),
                };

                if let Some(result) = self.invoke_hook(plugin, kuria_plugin::ON_SMTP_TO, &args)
                    && result.reject
                {
                    return Some(result);
                }
            }
        }
        None
    }

    /// Call `on_smtp_data` for all plugins. Returns the first rejection or modification.
    pub fn call_smtp_data(
        &self,
        sender: &str,
        recipients: &[String],
        raw_message: &[u8],
        peer_addr: &str,
        is_tls: bool,
    ) -> Option<HookResult> {
        let sender_cstr = CString::new(sender).unwrap_or_default();
        let peer_cstr = CString::new(peer_addr).unwrap_or_default();

        // Build C strings for recipients
        let rcpt_cstrs: Vec<CString> = recipients
            .iter()
            .map(|r| CString::new(r.as_str()).unwrap_or_default())
            .collect();
        let rcpt_ptrs: Vec<*const std::os::raw::c_char> =
            rcpt_cstrs.iter().map(|c| c.as_ptr()).collect();

        for plugin in &self.plugins {
            unsafe {
                let args = CHookArgs {
                    peer_addr: peer_cstr.as_ptr(),
                    is_tls,
                    sender: sender_cstr.as_ptr(),
                    recipient: std::ptr::null(),
                    recipients: rcpt_ptrs.as_ptr(),
                    recipients_len: rcpt_ptrs.len(),
                    raw_message: raw_message.as_ptr(),
                    raw_message_len: raw_message.len(),
                    config_json: std::ptr::null(),
                    method: std::ptr::null(),
                    path: std::ptr::null(),
                    headers_json: std::ptr::null(),
                    query: std::ptr::null(),
                };

                if let Some(result) = self.invoke_hook(plugin, kuria_plugin::ON_SMTP_DATA, &args)
                    && result.is_effective_smtp_data_result()
                {
                    return Some(result);
                }
            }
        }
        None
    }

    /// Call `on_web_request` for all plugins. Returns the first rejection, if any.
    pub fn call_web_request(
        &self,
        method: &str,
        path: &str,
        headers_json: &str,
        query: &str,
    ) -> Option<HookResult> {
        let method_cstr = CString::new(method).unwrap_or_default();
        let path_cstr = CString::new(path).unwrap_or_default();
        let headers_cstr = CString::new(headers_json).unwrap_or_default();
        let query_cstr = CString::new(query).unwrap_or_default();

        for plugin in &self.plugins {
            unsafe {
                let args = CHookArgs {
                    peer_addr: std::ptr::null(),
                    is_tls: false,
                    sender: std::ptr::null(),
                    recipient: std::ptr::null(),
                    recipients: std::ptr::null(),
                    recipients_len: 0,
                    raw_message: std::ptr::null(),
                    raw_message_len: 0,
                    config_json: std::ptr::null(),
                    method: method_cstr.as_ptr(),
                    path: path_cstr.as_ptr(),
                    headers_json: headers_cstr.as_ptr(),
                    query: query_cstr.as_ptr(),
                };

                if let Some(result) = self.invoke_hook(plugin, kuria_plugin::ON_WEB_REQUEST, &args)
                    && result.reject
                {
                    return Some(result);
                }
            }
        }
        None
    }

    /// Internal: call a hook on a single plugin and convert the C result to Rust.
    unsafe fn invoke_hook(
        &self,
        plugin: &PluginInstance,
        hook_id: u32,
        args: &CHookArgs,
    ) -> Option<HookResult> {
        unsafe {
            let result_ptr = ((*plugin.vtable).hook)(hook_id, args);
            if result_ptr.is_null() {
                return None;
            }

            let result = c_to_hook_result(result_ptr);
            ((*plugin.vtable).free_result)(result_ptr);
            Some(result)
        }
    }
}

/// Safe Rust representation of a hook result returned by a plugin.
#[derive(Debug, Clone)]
pub struct HookResult {
    pub action: u32,
    pub reject: bool,
    pub reject_message: Option<String>,
    pub modified_message: Option<Vec<u8>>,
    pub set_headers: Vec<(String, String)>,
    pub mailbox: Option<String>,
}

impl HookResult {
    fn is_effective_smtp_data_result(&self) -> bool {
        self.reject
            || self.action == kuria_plugin::ACTION_MODIFIED
            || self.modified_message.is_some()
            || !self.set_headers.is_empty()
            || self.mailbox.is_some()
    }
}

/// Convert a C `CHookResult` to a safe Rust `HookResult`. Copies all data so
/// the C result can be freed immediately after.
unsafe fn c_to_hook_result(ptr: *const CHookResult) -> HookResult {
    unsafe {
        let r = &*ptr;

        let reject_message = read_cstr(r.reject_message);
        let mailbox = read_cstr(r.mailbox);

        let modified_message = if !r.modified_message.is_null() && r.modified_message_len > 0 {
            Some(std::slice::from_raw_parts(r.modified_message, r.modified_message_len).to_vec())
        } else {
            None
        };

        let set_headers = if !r.set_headers.is_null() && r.set_headers_len > 0 {
            let entries = std::slice::from_raw_parts(r.set_headers, r.set_headers_len);
            entries
                .iter()
                .map(|e| {
                    let name = read_cstr(e.name).unwrap_or_default();
                    let value = read_cstr(e.value).unwrap_or_default();
                    (name, value)
                })
                .collect()
        } else {
            Vec::new()
        };

        HookResult {
            action: r.action,
            reject: r.reject,
            reject_message,
            modified_message,
            set_headers,
            mailbox,
        }
    }
}

/// Read a `*const c_char` to an Option<String>. Returns None if null.
unsafe fn read_cstr(ptr: *const std::os::raw::c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    unsafe {
        std::ffi::CStr::from_ptr(ptr)
            .to_str()
            .ok()
            .map(|s| s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smtp_data_results_are_effective_when_they_change_delivery() {
        let mut result = HookResult {
            action: kuria_plugin::ACTION_ACCEPT,
            reject: false,
            reject_message: None,
            modified_message: None,
            set_headers: Vec::new(),
            mailbox: None,
        };
        assert!(!result.is_effective_smtp_data_result());

        result
            .set_headers
            .push(("X-Test".to_string(), "ok".to_string()));
        assert!(result.is_effective_smtp_data_result());

        result.set_headers.clear();
        result.mailbox = Some("Spam".to_string());
        assert!(result.is_effective_smtp_data_result());

        result.mailbox = None;
        result.modified_message = Some(b"message".to_vec());
        assert!(result.is_effective_smtp_data_result());
    }
}
