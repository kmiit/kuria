//! # Kuria Plugin SDK
//!
//! This crate provides the types and macros needed to write plugins for the
//! Kuria Mail Server. Plugins are compiled as `cdylib` crates and loaded at
//! runtime by the host.
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use kuria_plugin::*;
//!
//! struct MyPlugin;
//!
//! impl Plugin for MyPlugin {
//!     fn metadata(&self) -> PluginMetadata {
//!         PluginMetadata {
//!             name: "my-plugin",
//!             version: "0.1.0",
//!             description: "A custom plugin",
//!         }
//!     }
//!
//!     fn on_init(&self, _ctx: &PluginContext) -> HookResult {
//!         HookResult::accept()
//!     }
//!
//!     fn on_smtp_data(&self, args: &SmtpDataArgs) -> HookResult {
//!         // Inspect or modify incoming email
//!         HookResult::accept()
//!     }
//! }
//!
//! declare_plugin!(MyPlugin);
//! ```

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

// ─── ABI Version ────────────────────────────────────────────────────────────

/// Current plugin ABI version. Bump when the repr(C) layout changes.
pub const PLUGIN_ABI_VERSION: u32 = 1;

// ─── Hook IDs ───────────────────────────────────────────────────────────────

pub const ON_INIT: u32 = 0;
pub const ON_SHUTDOWN: u32 = 1;
pub const ON_SMTP_CONNECT: u32 = 10;
pub const ON_SMTP_FROM: u32 = 11;
pub const ON_SMTP_TO: u32 = 12;
pub const ON_SMTP_DATA: u32 = 13;
pub const ON_WEB_REQUEST: u32 = 20;

// ─── Action Codes ───────────────────────────────────────────────────────────

pub const ACTION_ACCEPT: u32 = 0;
pub const ACTION_REJECT: u32 = 1;
pub const ACTION_MODIFIED: u32 = 2;

// ─── repr(C) Types ──────────────────────────────────────────────────────────

/// Metadata about a plugin, returned by the vtable.
#[repr(C)]
pub struct CPluginMetadata {
    pub name: *const c_char,
    pub version: *const c_char,
    pub description: *const c_char,
}

/// A single header entry for `set_headers`.
#[repr(C)]
pub struct CHeaderEntry {
    pub name: *mut c_char,
    pub value: *mut c_char,
}

/// Shared args struct passed to all hooks. Fields used depend on the hook ID.
/// The host fills relevant fields; the plugin reads only the fields for its hook.
#[repr(C)]
pub struct CHookArgs {
    // SMTP fields
    pub peer_addr: *const c_char,
    pub is_tls: bool,
    pub sender: *const c_char,
    pub recipient: *const c_char,
    pub recipients: *const *const c_char,
    pub recipients_len: usize,
    pub raw_message: *const u8,
    pub raw_message_len: usize,
    // Config / context
    pub config_json: *const c_char,
    // Web fields
    pub method: *const c_char,
    pub path: *const c_char,
    pub headers_json: *const c_char,
    pub query: *const c_char,
}

/// Result returned by a hook. The plugin allocates string/byte fields via
/// `CString::into_raw` / `Vec::into_raw`. The host frees them by calling
/// `PluginVTable.free_result`.
#[repr(C)]
pub struct CHookResult {
    pub action: u32,
    pub reject: bool,
    pub reject_message: *mut c_char,
    pub modified_message: *mut u8,
    pub modified_message_len: usize,
    pub set_headers: *mut CHeaderEntry,
    pub set_headers_len: usize,
    pub mailbox: *mut c_char,
}

/// Function pointer table returned by `kuria_plugin_create()`.
#[repr(C)]
pub struct PluginVTable {
    pub version: u32,
    pub metadata: *const CPluginMetadata,
    pub init: extern "C" fn(*const CHookArgs) -> *mut CHookResult,
    pub hook: extern "C" fn(hook_id: u32, *const CHookArgs) -> *mut CHookResult,
    pub shutdown: extern "C" fn() -> *mut CHookResult,
    pub free_result: extern "C" fn(*mut CHookResult),
}

// ─── Safe Rust Types (for Plugin trait) ─────────────────────────────────────

/// Safe Rust representation of plugin metadata.
pub struct PluginMetadata {
    pub name: &'static str,
    pub version: &'static str,
    pub description: &'static str,
}

/// Context passed to `on_init`.
pub struct PluginContext {
    pub config_json: String,
}

/// Safe args for `on_smtp_connect`.
pub struct SmtpConnectArgs {
    pub peer_addr: String,
    pub is_tls: bool,
}

/// Safe args for `on_smtp_from`.
pub struct SmtpFromArgs {
    pub sender: String,
    pub peer_addr: String,
}

/// Safe args for `on_smtp_to`.
pub struct SmtpToArgs {
    pub recipient: String,
    pub sender: String,
    pub peer_addr: String,
}

/// Safe args for `on_smtp_data`.
pub struct SmtpDataArgs {
    pub sender: String,
    pub recipients: Vec<String>,
    pub raw_message: Vec<u8>,
    pub peer_addr: String,
}

/// Safe args for `on_web_request`.
pub struct WebRequestArgs {
    pub method: String,
    pub path: String,
    pub headers_json: String,
    pub query: String,
}

/// Safe result returned by hook methods.
pub struct HookResult {
    pub action: u32,
    pub reject: bool,
    pub reject_message: Option<String>,
    pub modified_message: Option<Vec<u8>>,
    pub set_headers: Vec<(String, String)>,
    pub mailbox: Option<String>,
}

impl HookResult {
    pub fn accept() -> Self {
        Self {
            action: ACTION_ACCEPT,
            reject: false,
            reject_message: None,
            modified_message: None,
            set_headers: Vec::new(),
            mailbox: None,
        }
    }

    pub fn reject(msg: impl Into<String>) -> Self {
        Self {
            action: ACTION_REJECT,
            reject: true,
            reject_message: Some(msg.into()),
            modified_message: None,
            set_headers: Vec::new(),
            mailbox: None,
        }
    }

    pub fn modified() -> Self {
        Self {
            action: ACTION_MODIFIED,
            reject: false,
            reject_message: None,
            modified_message: None,
            set_headers: Vec::new(),
            mailbox: None,
        }
    }
}

// ─── Plugin Trait ───────────────────────────────────────────────────────────

/// Trait that plugins implement. All methods have default implementations that
/// return `HookResult::accept()`, so plugins only need to override the hooks
/// they care about.
pub trait Plugin: Send + Sync {
    fn metadata(&self) -> PluginMetadata;

    fn on_init(&self, _ctx: &PluginContext) -> HookResult {
        HookResult::accept()
    }

    fn on_shutdown(&self) -> HookResult {
        HookResult::accept()
    }

    fn on_smtp_connect(&self, _args: &SmtpConnectArgs) -> HookResult {
        HookResult::accept()
    }

    fn on_smtp_from(&self, _args: &SmtpFromArgs) -> HookResult {
        HookResult::accept()
    }

    fn on_smtp_to(&self, _args: &SmtpToArgs) -> HookResult {
        HookResult::accept()
    }

    fn on_smtp_data(&self, _args: &SmtpDataArgs) -> HookResult {
        HookResult::accept()
    }

    fn on_web_request(&self, _args: &WebRequestArgs) -> HookResult {
        HookResult::accept()
    }
}

// ─── Helper Functions ───────────────────────────────────────────────────────

/// Safely convert a C string pointer to a Rust String. Returns empty string
/// if the pointer is null or contains invalid UTF-8.
pub fn cstr_to_string(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .unwrap_or("")
        .to_string()
}

/// Convert a Rust String to a `*mut c_char` (allocated, caller must free).
pub fn string_to_cstr(s: String) -> *mut c_char {
    CString::new(s).unwrap_or_default().into_raw()
}

// ─── Internal: Safe → repr(C) Conversion ────────────────────────────────────

/// Build a `CPluginMetadata` from `PluginMetadata`. The CStrings are leaked
/// intentionally — they live for the duration of the plugin.
pub fn build_c_metadata(meta: &PluginMetadata) -> CPluginMetadata {
    CPluginMetadata {
        name: CString::new(meta.name).unwrap().into_raw(),
        version: CString::new(meta.version).unwrap().into_raw(),
        description: CString::new(meta.description).unwrap().into_raw(),
    }
}

/// Convert a safe `HookResult` to a boxed `CHookResult` (returns raw pointer
/// for the host to free via `free_result`).
pub fn hook_result_to_c(result: HookResult) -> *mut CHookResult {
    let reject_message = result
        .reject_message
        .map(string_to_cstr)
        .unwrap_or(std::ptr::null_mut());

    let (modified_message, modified_message_len) = match result.modified_message {
        Some(mut v) => {
            v.shrink_to_fit();
            let len = v.len();
            let ptr = v.as_mut_ptr();
            std::mem::forget(v);
            (ptr, len)
        }
        None => (std::ptr::null_mut(), 0),
    };

    let (set_headers, set_headers_len) = if result.set_headers.is_empty() {
        (std::ptr::null_mut(), 0)
    } else {
        let entries: Vec<CHeaderEntry> = result
            .set_headers
            .into_iter()
            .map(|(k, v)| CHeaderEntry {
                name: string_to_cstr(k),
                value: string_to_cstr(v),
            })
            .collect();
        let mut entries = entries;
        let len = entries.len();
        let ptr = entries.as_mut_ptr();
        std::mem::forget(entries);
        (ptr, len)
    };

    let mailbox = result
        .mailbox
        .map(string_to_cstr)
        .unwrap_or(std::ptr::null_mut());

    Box::into_raw(Box::new(CHookResult {
        action: result.action,
        reject: result.reject,
        reject_message,
        modified_message,
        modified_message_len,
        set_headers,
        set_headers_len,
        mailbox,
    }))
}

/// Free a `CHookResult` allocated by `hook_result_to_c`.
pub fn free_c_hook_result(result: *mut CHookResult) {
    if result.is_null() {
        return;
    }
    unsafe {
        let r = &mut *result;
        // Free reject_message
        if !r.reject_message.is_null() {
            drop(CString::from_raw(r.reject_message));
        }
        // Free modified_message
        if !r.modified_message.is_null() && r.modified_message_len > 0 {
            drop(Vec::from_raw_parts(
                r.modified_message,
                r.modified_message_len,
                r.modified_message_len,
            ));
        }
        // Free set_headers
        if !r.set_headers.is_null() && r.set_headers_len > 0 {
            let headers = Vec::from_raw_parts(r.set_headers, r.set_headers_len, r.set_headers_len);
            for h in headers {
                if !h.name.is_null() {
                    drop(CString::from_raw(h.name));
                }
                if !h.value.is_null() {
                    drop(CString::from_raw(h.value));
                }
            }
        }
        // Free mailbox
        if !r.mailbox.is_null() {
            drop(CString::from_raw(r.mailbox));
        }
        // Free the struct itself
        drop(Box::from_raw(result));
    }
}

// ─── declare_plugin! Macro ─────────────────────────────────────────────────

/// Declare a plugin. This generates all the FFI exports needed by the host.
///
/// Usage:
/// ```rust,no_run
/// use kuria_plugin::*;
///
/// struct MyPlugin;
/// impl Plugin for MyPlugin { ... }
///
/// declare_plugin!(MyPlugin);
/// ```
///
/// The generated code:
/// - Exports `kuria_plugin_create()` which returns a `*const PluginVTable`
/// - Implements the FFI bridge: unsafe repr(C) → safe Rust types → Plugin trait
/// - Handles memory allocation/deallocation across the FFI boundary
#[macro_export]
macro_rules! declare_plugin {
    ($plugin_type:ty) => {
        // We use a static to hold the vtable (it lives for the process lifetime).
        static mut PLUGIN_VTABLE: Option<$crate::PluginVTable> = None;
        static mut PLUGIN_METADATA: Option<$crate::CPluginMetadata> = None;

        extern "C" fn __kuria_init(args: *const $crate::CHookArgs) -> *mut $crate::CHookResult {
            unsafe {
                let ctx = $crate::PluginContext {
                    config_json: $crate::cstr_to_string((*args).config_json),
                };
                let plugin = __PLUGIN_INSTANCE.as_ref().unwrap();
                $crate::hook_result_to_c(plugin.on_init(&ctx))
            }
        }

        extern "C" fn __kuria_shutdown() -> *mut $crate::CHookResult {
            unsafe {
                let plugin = __PLUGIN_INSTANCE.as_ref().unwrap();
                $crate::hook_result_to_c(plugin.on_shutdown())
            }
        }

        extern "C" fn __kuria_hook(
            hook_id: u32,
            args: *const $crate::CHookArgs,
        ) -> *mut $crate::CHookResult {
            unsafe {
                let plugin = __PLUGIN_INSTANCE.as_ref().unwrap();
                let a = &*args;

                let result = match hook_id {
                    $crate::ON_SMTP_CONNECT => {
                        let smtp_args = $crate::SmtpConnectArgs {
                            peer_addr: $crate::cstr_to_string(a.peer_addr),
                            is_tls: a.is_tls,
                        };
                        plugin.on_smtp_connect(&smtp_args)
                    }
                    $crate::ON_SMTP_FROM => {
                        let smtp_args = $crate::SmtpFromArgs {
                            sender: $crate::cstr_to_string(a.sender),
                            peer_addr: $crate::cstr_to_string(a.peer_addr),
                        };
                        plugin.on_smtp_from(&smtp_args)
                    }
                    $crate::ON_SMTP_TO => {
                        let smtp_args = $crate::SmtpToArgs {
                            recipient: $crate::cstr_to_string(a.recipient),
                            sender: $crate::cstr_to_string(a.sender),
                            peer_addr: $crate::cstr_to_string(a.peer_addr),
                        };
                        plugin.on_smtp_to(&smtp_args)
                    }
                    $crate::ON_SMTP_DATA => {
                        let recipients = if !a.recipients.is_null() && a.recipients_len > 0 {
                            std::slice::from_raw_parts(a.recipients, a.recipients_len)
                                .iter()
                                .map(|&p| $crate::cstr_to_string(p))
                                .collect()
                        } else {
                            Vec::new()
                        };
                        let raw_message = if !a.raw_message.is_null() && a.raw_message_len > 0 {
                            std::slice::from_raw_parts(a.raw_message, a.raw_message_len).to_vec()
                        } else {
                            Vec::new()
                        };
                        let smtp_args = $crate::SmtpDataArgs {
                            sender: $crate::cstr_to_string(a.sender),
                            recipients,
                            raw_message,
                            peer_addr: $crate::cstr_to_string(a.peer_addr),
                        };
                        plugin.on_smtp_data(&smtp_args)
                    }
                    $crate::ON_WEB_REQUEST => {
                        let web_args = $crate::WebRequestArgs {
                            method: $crate::cstr_to_string(a.method),
                            path: $crate::cstr_to_string(a.path),
                            headers_json: $crate::cstr_to_string(a.headers_json),
                            query: $crate::cstr_to_string(a.query),
                        };
                        plugin.on_web_request(&web_args)
                    }
                    _ => $crate::HookResult::accept(),
                };
                $crate::hook_result_to_c(result)
            }
        }

        extern "C" fn __kuria_free_result(result: *mut $crate::CHookResult) {
            $crate::free_c_hook_result(result);
        }

        // Static plugin instance (leaked, lives for process lifetime).
        static mut __PLUGIN_INSTANCE: Option<$plugin_type> = None;

        #[no_mangle]
        pub extern "C" fn kuria_plugin_create() -> *const $crate::PluginVTable {
            unsafe {
                // Create the plugin instance.
                __PLUGIN_INSTANCE = Some(<$plugin_type>::default());

                // Build metadata.
                let plugin = __PLUGIN_INSTANCE.as_ref().unwrap();
                let meta = plugin.metadata();
                PLUGIN_METADATA = Some($crate::build_c_metadata(&meta));

                // Build vtable.
                PLUGIN_VTABLE = Some($crate::PluginVTable {
                    version: $crate::PLUGIN_ABI_VERSION,
                    metadata: PLUGIN_METADATA.as_ref().unwrap(),
                    init: __kuria_init,
                    hook: __kuria_hook,
                    shutdown: __kuria_shutdown,
                    free_result: __kuria_free_result,
                });

                PLUGIN_VTABLE.as_ref().unwrap()
            }
        }
    };
}
