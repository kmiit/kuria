use kuria_plugin::{CHookArgs, CHookResult, PLUGIN_ABI_VERSION, PluginVTable};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use tracing::{error, info, warn};

use crate::config::Config;

#[derive(Debug, Clone, Serialize)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub path: String,
    pub admin_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginApiResponse {
    pub status_code: u16,
    pub body_json: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginLoadError {
    pub path: String,
    pub error: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PluginOutboundEmail {
    pub from: String,
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    #[serde(default)]
    pub bcc: Vec<String>,
    pub subject: String,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PluginOutboundEmailResponse {
    #[serde(default)]
    pub outbound_emails: Vec<PluginOutboundEmail>,
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

        let (plugin_paths, discovery_errors) = configured_plugin_paths(plugins_config);
        load_errors.extend(discovery_errors);
        for path in &plugin_paths {
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
        let library_path = resolve_plugin_library_path(path)?;
        unsafe {
            let library = load_dynamic_library(&library_path)?;

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
            let admin_path = plugin_embedded_admin_ui_available(vtable).then(|| {
                format!(
                    "/plugin-assets/{}/index.html",
                    plugin_asset_path_segment(&name)
                )
            });

            Ok(PluginInstance {
                info: PluginInfo {
                    name,
                    version,
                    description,
                    path: library_path.to_string_lossy().to_string(),
                    admin_path,
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

    pub fn call_plugin_admin_asset(
        &self,
        plugin_name: &str,
        request_path: &str,
    ) -> Option<PluginApiResponse> {
        let plugin = self
            .plugins
            .iter()
            .find(|plugin| plugin.info.name.eq_ignore_ascii_case(plugin_name))?;
        plugin.info.admin_path.as_ref()?;
        let path = plugin_admin_asset_api_path(request_path)?;
        let method_cstr = CString::new("GET").ok()?;
        let path_cstr = CString::new(path).ok()?;
        let headers_cstr = CString::new("{}").ok()?;
        let empty_cstr = CString::new("").ok()?;

        let result = unsafe {
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
                query: empty_cstr.as_ptr(),
                body_json: empty_cstr.as_ptr(),
                user_json: empty_cstr.as_ptr(),
                event_json: std::ptr::null(),
            };

            self.invoke_hook(plugin, kuria_plugin::ON_PLUGIN_API, &args)
        }?;

        Some(PluginApiResponse {
            status_code: if result.status_code == 0 {
                200
            } else {
                result.status_code
            },
            body_json: result.response_json.unwrap_or_default(),
        })
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
                    body_json: std::ptr::null(),
                    user_json: std::ptr::null(),
                    event_json: std::ptr::null(),
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
                    body_json: std::ptr::null(),
                    user_json: std::ptr::null(),
                    event_json: std::ptr::null(),
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
                    body_json: std::ptr::null(),
                    user_json: std::ptr::null(),
                    event_json: std::ptr::null(),
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
                    body_json: std::ptr::null(),
                    user_json: std::ptr::null(),
                    event_json: std::ptr::null(),
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
                    body_json: std::ptr::null(),
                    user_json: std::ptr::null(),
                    event_json: std::ptr::null(),
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
                    body_json: std::ptr::null(),
                    user_json: std::ptr::null(),
                    event_json: std::ptr::null(),
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

    pub fn call_plugin_api(
        &self,
        plugin_name: &str,
        method: &str,
        path: &str,
        headers_json: &str,
        query: &str,
        body_json: &str,
        user_json: &str,
    ) -> Option<PluginApiResponse> {
        let plugin = self
            .plugins
            .iter()
            .find(|plugin| plugin.info.name.eq_ignore_ascii_case(plugin_name))?;
        let method_cstr = CString::new(method).unwrap_or_default();
        let path_cstr = CString::new(path).unwrap_or_default();
        let headers_cstr = CString::new(headers_json).unwrap_or_default();
        let query_cstr = CString::new(query).unwrap_or_default();
        let body_cstr = CString::new(body_json).unwrap_or_default();
        let user_cstr = CString::new(user_json).unwrap_or_default();

        let result = unsafe {
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
                body_json: body_cstr.as_ptr(),
                user_json: user_cstr.as_ptr(),
                event_json: std::ptr::null(),
            };

            self.invoke_hook(plugin, kuria_plugin::ON_PLUGIN_API, &args)
        };

        let result = result?;
        let body_json = result.response_json.unwrap_or_else(|| {
            json!({
                "error": "Plugin did not return a response",
            })
            .to_string()
        });
        Some(PluginApiResponse {
            status_code: if result.status_code == 0 {
                200
            } else {
                result.status_code
            },
            body_json,
        })
    }

    pub fn call_plugin_webhook(
        &self,
        plugin_name: &str,
        method: &str,
        path: &str,
        headers_json: &str,
        query: &str,
        body_json: &str,
    ) -> Option<PluginApiResponse> {
        let plugin = self
            .plugins
            .iter()
            .find(|plugin| plugin.info.name.eq_ignore_ascii_case(plugin_name))?;
        let method_cstr = CString::new(method).unwrap_or_default();
        let path_cstr = CString::new(path).unwrap_or_default();
        let headers_cstr = CString::new(headers_json).unwrap_or_default();
        let query_cstr = CString::new(query).unwrap_or_default();
        let body_cstr = CString::new(body_json).unwrap_or_default();

        let result = unsafe {
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
                body_json: body_cstr.as_ptr(),
                user_json: std::ptr::null(),
                event_json: std::ptr::null(),
            };

            self.invoke_hook(plugin, kuria_plugin::ON_PLUGIN_WEBHOOK, &args)
        };

        let result = result?;
        let body_json = result.response_json.unwrap_or_else(|| {
            json!({
                "error": "Plugin did not return a response",
            })
            .to_string()
        });
        Some(PluginApiResponse {
            status_code: if result.status_code == 0 {
                200
            } else {
                result.status_code
            },
            body_json,
        })
    }

    pub fn call_mail_delivered(&self, event_json: &str) {
        let event_cstr = CString::new(event_json).unwrap_or_default();

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
                    method: std::ptr::null(),
                    path: std::ptr::null(),
                    headers_json: std::ptr::null(),
                    query: std::ptr::null(),
                    body_json: std::ptr::null(),
                    user_json: std::ptr::null(),
                    event_json: event_cstr.as_ptr(),
                };

                let _ = self.invoke_hook(plugin, kuria_plugin::ON_MAIL_DELIVERED, &args);
            }
        }
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

fn resolve_plugin_library_path(path: &str) -> anyhow::Result<PathBuf> {
    let path = Path::new(path);
    if !path.exists() {
        return Err(anyhow::anyhow!(
            "Plugin library does not exist: {}",
            path.display()
        ));
    }
    if !path.is_file() {
        return Err(anyhow::anyhow!(
            "Plugin library path is not a file: {}",
            path.display()
        ));
    }

    path.canonicalize().map_err(|error| {
        anyhow::anyhow!(
            "Failed to resolve plugin library path {}: {}",
            path.display(),
            error
        )
    })
}

#[cfg(windows)]
unsafe fn load_dynamic_library(path: &Path) -> anyhow::Result<libloading::Library> {
    use libloading::os::windows::{
        LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
        Library as WindowsLibrary,
    };

    unsafe {
        WindowsLibrary::load_with_flags(
            path,
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
        )
        .map(Into::into)
        .map_err(|error| {
            anyhow::anyhow!(
                "Failed to load library: {}. Make sure any dependent DLLs are next to the plugin DLL or in a system DLL directory.",
                error
            )
        })
    }
}

#[cfg(not(windows))]
unsafe fn load_dynamic_library(path: &Path) -> anyhow::Result<libloading::Library> {
    unsafe {
        libloading::Library::new(path)
            .map_err(|error| anyhow::anyhow!("Failed to load library: {}", error))
    }
}

fn plugin_embedded_admin_ui_available(vtable: *const PluginVTable) -> bool {
    unsafe {
        invoke_embedded_admin_asset(vtable, "/admin/manifest")
            .map(|result| {
                (200..300).contains(&result.status_code) && result.response_json.is_some()
            })
            .unwrap_or(false)
    }
}

unsafe fn invoke_embedded_admin_asset(
    vtable: *const PluginVTable,
    path: &str,
) -> Option<HookResult> {
    let method_cstr = CString::new("GET").ok()?;
    let path_cstr = CString::new(path).ok()?;
    let headers_cstr = CString::new("{}").ok()?;
    let empty_cstr = CString::new("").ok()?;

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
            query: empty_cstr.as_ptr(),
            body_json: empty_cstr.as_ptr(),
            user_json: empty_cstr.as_ptr(),
            event_json: std::ptr::null(),
        };
        let result_ptr = ((*vtable).hook)(kuria_plugin::ON_PLUGIN_API, &args);
        if result_ptr.is_null() {
            return None;
        }

        let result = c_to_hook_result(result_ptr);
        ((*vtable).free_result)(result_ptr);
        Some(result)
    }
}

fn plugin_admin_asset_api_path(request_path: &str) -> Option<String> {
    let request_path = request_path.trim_matches('/');
    if request_path.is_empty() {
        return Some("/admin/".to_string());
    }

    let mut segments = Vec::new();
    for segment in request_path.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." || segment.contains('\\') {
            return None;
        }
        segments.push(segment);
    }

    if segments.is_empty() {
        Some("/admin/".to_string())
    } else {
        Some(format!("/admin/{}", segments.join("/")))
    }
}

fn plugin_asset_path_segment(name: &str) -> String {
    name.replace(['/', '\\'], "_")
}

fn configured_plugin_paths(
    config: &crate::config::PluginsConfig,
) -> (Vec<String>, Vec<PluginLoadError>) {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    let mut load_errors = Vec::new();

    for path in &config.paths {
        push_plugin_path(&mut paths, &mut seen, path.clone());
    }

    if let Some(directory) = &config.directory {
        let dynamic_ext = std::env::consts::DLL_EXTENSION;
        match std::fs::read_dir(directory) {
            Ok(entries) => {
                let mut discovered = Vec::new();
                for entry in entries {
                    let entry = match entry {
                        Ok(entry) => entry,
                        Err(error) => {
                            warn!(
                                "Failed to read plugin directory entry in {}: {}",
                                directory, error
                            );
                            load_errors.push(PluginLoadError {
                                path: directory.clone(),
                                error: format!("Failed to read plugin directory entry: {error}"),
                            });
                            continue;
                        }
                    };

                    let path = entry.path();
                    if !is_dynamic_library_file(&path, dynamic_ext) {
                        continue;
                    }
                    discovered.push(path.to_string_lossy().to_string());
                }

                discovered.sort();
                for path in discovered {
                    push_plugin_path(&mut paths, &mut seen, path);
                }
            }
            Err(error) => {
                warn!("Failed to scan plugin directory {}: {}", directory, error);
                load_errors.push(PluginLoadError {
                    path: directory.clone(),
                    error: format!("Failed to scan plugin directory: {error}"),
                });
            }
        }
    }

    (paths, load_errors)
}

fn is_dynamic_library_file(path: &Path, dynamic_ext: &str) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case(dynamic_ext))
}

fn push_plugin_path(paths: &mut Vec<String>, seen: &mut HashSet<String>, path: String) {
    if seen.insert(plugin_path_key(&path)) {
        paths.push(path);
    }
}

fn plugin_path_key(path: &str) -> String {
    let key = Path::new(path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(path))
        .to_string_lossy()
        .to_string();

    #[cfg(windows)]
    {
        key.to_ascii_lowercase()
    }

    #[cfg(not(windows))]
    {
        key
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
    pub response_json: Option<String>,
    pub status_code: u16,
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
        let response_json = read_cstr(r.response_json);

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
            response_json,
            status_code: r.status_code,
        }
    }
}

pub fn mail_delivered_event_json(email: &crate::db::models::Email, user_email: &str) -> String {
    let recipients: Vec<String> = serde_json::from_str(&email.recipients).unwrap_or_default();
    json!({
        "kind": "mail_delivered",
        "email_id": email.id,
        "user_id": email.user_id,
        "user_email": user_email,
        "mailbox": email.mailbox.as_deref().unwrap_or("INBOX"),
        "sender": email.sender,
        "recipients": recipients,
        "subject": email.subject,
        "body_preview": body_preview(email),
        "created_at": email.created_at,
    })
    .to_string()
}

fn body_preview(email: &crate::db::models::Email) -> String {
    let body = email
        .body_text
        .as_deref()
        .or(email.body_html.as_deref())
        .unwrap_or("");
    body.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(240)
        .collect()
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
            response_json: None,
            status_code: 200,
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

    #[test]
    fn configured_plugin_paths_discovers_libraries_from_directory_without_explicit_paths() {
        let dir = unique_test_dir("plugin-discovery");
        std::fs::create_dir_all(&dir).expect("create temp plugin dir");
        let plugin = dir.join(format!("sample.{}", std::env::consts::DLL_EXTENSION));
        let ignored = dir.join("ignored.txt");
        std::fs::write(&plugin, b"not a real plugin").expect("write plugin placeholder");
        std::fs::write(ignored, b"ignored").expect("write ignored file");

        let config = crate::config::PluginsConfig {
            enabled: true,
            paths: Vec::new(),
            directory: Some(dir.to_string_lossy().to_string()),
        };

        let (paths, errors) = configured_plugin_paths(&config);

        assert!(errors.is_empty());
        assert_eq!(paths, vec![plugin.to_string_lossy().to_string()]);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn configured_plugin_paths_deduplicates_explicit_and_discovered_paths() {
        let dir = unique_test_dir("plugin-dedupe");
        std::fs::create_dir_all(&dir).expect("create temp plugin dir");
        let plugin = dir.join(format!("sample.{}", std::env::consts::DLL_EXTENSION));
        std::fs::write(&plugin, b"not a real plugin").expect("write plugin placeholder");

        let plugin_path = plugin.to_string_lossy().to_string();
        let config = crate::config::PluginsConfig {
            enabled: true,
            paths: vec![plugin_path.clone()],
            directory: Some(dir.to_string_lossy().to_string()),
        };

        let (paths, errors) = configured_plugin_paths(&config);

        assert!(errors.is_empty());
        assert_eq!(paths, vec![plugin_path]);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn plugin_admin_asset_paths_are_normalized() {
        assert_eq!(plugin_admin_asset_api_path(""), Some("/admin/".to_string()));
        assert_eq!(
            plugin_admin_asset_api_path("app.js"),
            Some("/admin/app.js".to_string())
        );
        assert_eq!(
            plugin_admin_asset_api_path("/nested/route/"),
            Some("/admin/nested/route".to_string())
        );
        assert!(plugin_admin_asset_api_path("../secret.txt").is_none());
        assert!(plugin_admin_asset_api_path("..\\secret.txt").is_none());
    }

    fn unique_test_dir(name: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("kuria-{name}-{}-{nonce}", std::process::id()))
    }
}
