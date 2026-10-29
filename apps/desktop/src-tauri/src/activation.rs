//! The activation token for the programs the app starts. The desktop raises a window only for a
//! request that carries one, and on Wayland the token comes from the compositor through GTK's
//! display connection (see `patchwork_server::launch`).

use tauri::AppHandle;

/// The token for the next program the app starts, or None off Wayland.
///
/// The GTK main thread owns the display's connection, so the token is taken there and this waits
/// for it. A sync command runs on that thread and must not await this, or it would wait on itself.
#[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
pub(crate) async fn token(app: &AppHandle) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let (tx, rx) = tokio::sync::oneshot::channel();
        app.run_on_main_thread(move || {
            let _ = tx.send(wayland::token());
        })
        .ok()?;
        rx.await.ok().flatten()
    }
    #[cfg(not(target_os = "linux"))]
    None
}

#[cfg(target_os = "linux")]
mod wayland {
    use std::ffi::c_void;

    use gdk::glib::{ObjectExt, ObjectType};

    unsafe extern "C" {
        fn gdk_wayland_display_get_wl_display(display: *mut gdk::ffi::GdkDisplay) -> *mut c_void;
    }

    /// Runs on the GTK main thread only.
    pub(super) fn token() -> Option<String> {
        let display = gdk::Display::default()?;
        if display.type_().name() != "GdkWaylandDisplay" {
            return None;
        }
        // SAFETY: GDK's display lives as long as the app, and this runs on the thread that owns it.
        let wl_display = unsafe { gdk_wayland_display_get_wl_display(display.as_ptr()) };
        if wl_display.is_null() {
            return None;
        }
        // SAFETY: `wl_display` is live, and nothing else reads its connection while the main thread
        // runs this.
        unsafe { patchwork_server::system::launch::wayland_token(wl_display) }
    }
}
