//! O ext-session-lock-v1 pelo gtk4-layer-shell 1.2 ou mais novo (o GtkSessionLockInstance), achado na
//! hora (dlsym) na biblioteca que o binário já carrega. O crate gtk4-session-lock pede um gtk4 mais novo
//! que o 0.9 do workspace, e o Debian 13 tem o gtk4-layer-shell 1.0.4, sem o bloqueio: lá, este binário
//! diz que não dá e sai com 1, e o sway-lock passa ao sway-lock-gtk (Python). O 1.2 é o que tem o sinal
//! ::monitor; o Ubuntu 26.04 tem o 1.3.
use std::ffi::{c_void, CStr};

use gtk4::glib::translate::{FromGlibPtrFull, ToGlibPtr};
use gtk4::prelude::*;
use gtk4::{gdk, glib};

type NewFn = unsafe extern "C" fn() -> *mut glib::gobject_ffi::GObject;
type LockFn = unsafe extern "C" fn(*mut glib::gobject_ffi::GObject) -> glib::ffi::gboolean;
type UnlockFn = unsafe extern "C" fn(*mut glib::gobject_ffi::GObject);
type AssignFn = unsafe extern "C" fn(*mut glib::gobject_ffi::GObject, *mut gtk4::ffi::GtkWindow, *mut gdk::ffi::GdkMonitor);
type SupportedFn = unsafe extern "C" fn() -> glib::ffi::gboolean;

pub const MINIMUM: (u32, u32) = (1, 2);

#[derive(Clone, Copy)]
pub struct Api {
    new: NewFn,
    lock: LockFn,
    unlock: UnlockFn,
    assign: AssignFn,
    supported: SupportedFn,
}

unsafe fn symbol(name: &CStr) -> Result<*mut c_void, String> {
    let found = libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr());
    if found.is_null() {
        Err(format!("the gtk4-layer-shell library has no {}", name.to_string_lossy()))
    } else {
        Ok(found)
    }
}

/// As funções do bloqueio, ou por que não há. Não precisa do Wayland: serve ao --check.
pub fn api() -> Result<Api, String> {
    // As versões também prendem a biblioteca ao binário (o ligador tiraria uma que não fosse usada)
    let version = unsafe { (gtk4_layer_shell_sys::gtk_layer_get_major_version(), gtk4_layer_shell_sys::gtk_layer_get_minor_version()) };
    if version < MINIMUM {
        return Err(format!(
            "gtk4-layer-shell {}.{} has no session lock (it needs {}.{} or newer)",
            version.0, version.1, MINIMUM.0, MINIMUM.1
        ));
    }
    unsafe {
        Ok(Api {
            new: std::mem::transmute::<*mut c_void, NewFn>(symbol(c"gtk_session_lock_instance_new")?),
            lock: std::mem::transmute::<*mut c_void, LockFn>(symbol(c"gtk_session_lock_instance_lock")?),
            unlock: std::mem::transmute::<*mut c_void, UnlockFn>(symbol(c"gtk_session_lock_instance_unlock")?),
            assign: std::mem::transmute::<*mut c_void, AssignFn>(symbol(c"gtk_session_lock_instance_assign_window_to_monitor")?),
            supported: std::mem::transmute::<*mut c_void, SupportedFn>(symbol(c"gtk_session_lock_is_supported")?),
        })
    }
}

/// Um GtkSessionLockInstance: os sinais (locked, failed, unlocked, monitor) pelo glib::Object.
pub struct Instance {
    api: Api,
    pub object: glib::Object,
}

impl Api {
    /// Se o compositor tem o ext-session-lock-v1. Depois do gtk4::init.
    pub fn is_supported(&self) -> bool {
        unsafe { (self.supported)() != 0 }
    }

    pub fn instance(&self) -> Instance {
        let object = unsafe { glib::Object::from_glib_full((self.new)()) };
        Instance { api: *self, object }
    }
}

impl Instance {
    /// Falso numa falha imediata (o ::failed também vem).
    pub fn lock(&self) -> bool {
        unsafe { (self.api.lock)(self.object.as_ptr()) != 0 }
    }

    pub fn unlock(&self) {
        unsafe { (self.api.unlock)(self.object.as_ptr()) }
    }

    /// Uma janela nova (não realizada) para o monitor. Ela é desmapeada e destruída pela biblioteca
    /// quando o monitor sai ou o bloqueio acaba.
    pub fn assign(&self, window: &gtk4::Window, monitor: &gdk::Monitor) {
        unsafe { (self.api.assign)(self.object.as_ptr(), window.to_glib_none().0, monitor.to_glib_none().0) }
    }

    pub fn connect(&self, signal: &str, f: impl Fn(&[glib::Value]) + 'static) {
        self.object.connect_local(signal, false, move |values| {
            f(values);
            None
        });
    }
}
