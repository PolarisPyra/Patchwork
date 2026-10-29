//! Starting other programs on the user's desktop: opening files, links, and editors, and showing
//! a file in the file manager. On Wayland a window a program opens is only raised if the request
//! carries an activation token from the compositor, so these pass one on when there is one.

#[cfg(target_os = "linux")]
use crate::Error;
use std::process::Command;
#[cfg(target_os = "linux")]
use std::process::Stdio;

/// Passes `token` to a program the app starts. The desktop reads it from `XDG_ACTIVATION_TOKEN`,
/// and does not raise the window the program opens without it.
pub fn pass_token(cmd: &mut Command, token: Option<&str>) {
    if let Some(token) = token {
        cmd.env("XDG_ACTIVATION_TOKEN", token);
    }
}

/// Opens `target`, a path or a link, in the user's default app through `xdg-open`. Its launcher is
/// reaped in the background.
#[cfg(target_os = "linux")]
pub fn xdg_open(target: &std::ffi::OsStr, token: Option<&str>) -> std::io::Result<()> {
    let mut cmd = crate::system::host_command("xdg-open");
    pass_token(&mut cmd, token);
    let mut child = cmd
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// Shows `path` selected in the file manager, through `org.freedesktop.FileManager1`, which takes
/// the token as its startup id. Without that service, opens the folder holding `path`.
#[cfg(target_os = "linux")]
pub fn reveal(path: &std::path::Path, token: Option<&str>) -> Result<(), Error> {
    let uri = url::Url::from_file_path(path)
        .map_err(|()| format!("Could not show {}", path.display()))?;
    let uris = [uri.as_str()];
    let shown = zbus::blocking::Connection::session().and_then(|conn| {
        conn.call_method(
            Some("org.freedesktop.FileManager1"),
            "/org/freedesktop/FileManager1",
            Some("org.freedesktop.FileManager1"),
            "ShowItems",
            &(&uris[..], token.unwrap_or("")),
        )
    });
    if shown.is_ok() {
        return Ok(());
    }
    let dir = path.parent().unwrap_or(path);
    xdg_open(dir.as_os_str(), token)
        .map_err(|e| format!("Could not show {}: {e}", path.display()).into())
}

/// The activation token for the next program the app starts, taken from the Wayland display
/// `display`: the `wl_display` of the GDK connection.
///
/// # Safety
///
/// `display` must be a live `wl_display`, and no other thread may read from its connection while
/// this runs. The GTK main thread, which owns the connection, is the caller.
#[cfg(target_os = "linux")]
pub unsafe fn wayland_token(display: *mut std::ffi::c_void) -> Option<String> {
    // SAFETY: the caller upholds the contract above.
    match unsafe { request_token(display) } {
        Ok(token) => Some(token),
        Err(e) => {
            log::warn!("Could not get an activation token, so the window may open behind: {e}");
            None
        }
    }
}

#[cfg(target_os = "linux")]
unsafe fn request_token(
    display: *mut std::ffi::c_void,
) -> Result<String, Box<dyn std::error::Error>> {
    use wayland_client::{
        Connection, Dispatch, QueueHandle,
        backend::Backend,
        globals::{GlobalListContents, registry_queue_init},
        protocol::wl_registry,
    };
    use wayland_protocols::xdg::activation::v1::client::{
        xdg_activation_token_v1::{self, XdgActivationTokenV1},
        xdg_activation_v1::{self, XdgActivationV1},
    };

    #[derive(Default)]
    struct Token(Option<String>);

    impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Token {
        fn event(
            _: &mut Self,
            _: &wl_registry::WlRegistry,
            _: wl_registry::Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<XdgActivationV1, ()> for Token {
        fn event(
            _: &mut Self,
            _: &XdgActivationV1,
            _: xdg_activation_v1::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<XdgActivationTokenV1, ()> for Token {
        fn event(
            token: &mut Self,
            _: &XdgActivationTokenV1,
            event: xdg_activation_token_v1::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            if let xdg_activation_token_v1::Event::Done { token: value } = event {
                token.0 = Some(value);
            }
        }
    }

    // SAFETY: the caller guarantees `display` is live and that this thread owns its connection.
    let connection =
        Connection::from_backend(unsafe { Backend::from_foreign_display(display.cast()) });
    let (globals, mut queue) = registry_queue_init::<Token>(&connection)?;
    let qh = queue.handle();
    let activation: XdgActivationV1 = globals.bind(&qh, 1..=1, ())?;
    let request = activation.get_activation_token(&qh, ());
    request.commit();
    let mut token = Token::default();
    queue.roundtrip(&mut token)?;
    request.destroy();
    activation.destroy();
    token
        .0
        .ok_or_else(|| "The compositor sent no activation token".into())
}
