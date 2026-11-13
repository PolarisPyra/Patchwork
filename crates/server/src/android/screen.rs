//! An emulator's screen in the panel: its frames streamed over the emulator's gRPC interface, and
//! taps, wheel turns and keys sent back the same way, which is how Android Studio shows one
//! inside its own window.
//!
//! An emulator with gRPC on writes its port and an access token to a discovery file named for
//! its process. Patchwork starts its emulators that way with their own window hidden; one started
//! elsewhere shows here too when it has gRPC on, as Android Studio's do.
//!
//! Each screen runs on a thread of its own with a small async runtime, which ends when the
//! [`Screen`] is dropped or the emulator closes.

// Rust guideline compliant 2026-10-09

use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::StreamExt;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use tokio::sync::watch;

use super::avd::ini;
use super::devices::{ADB_LIMIT, emulator_port};
use super::tool::{self, tool_command};
use super::grpc::{
    Client, Event, Image, ImageFormat, InputEvent, KEYPRESS, KeyboardEvent, MouseEvent, RGBA8888,
};
use super::types::{ScreenFrame, ScreenInput};
use crate::{Error, Server};

/// The latest frame, or why there will be no more.
pub(super) type Latest = Option<Result<Arc<ScreenFrame>, String>>;

/// Runs adb on the screen's device with the arguments given, for keys the emulator drops.
type Adb = Arc<dyn Fn(&[String]) -> Result<(), Error> + Send + Sync>;

/// How far a line of the wheel's turn scrolls, as a fraction of the screen's height. The emulator
/// takes no wheel, so a turn is a short drag.
const WHEEL_DRAG: f32 = 0.06;

/// The steps a wheel's drag moves in, and the time between them: slow enough, with a pause
/// before the finger lifts, that Android scrolls by the drag and does not fling.
const WHEEL_STEPS: u16 = 6;
const WHEEL_STEP: std::time::Duration = std::time::Duration::from_millis(12);

/// A running emulator's screen: its frames as they come, and the way to send it input.
#[derive(Debug)]
pub struct Screen {
    frames: watch::Receiver<Latest>,
    input: ScreenSender,
    display: (u32, u32),
}

/// Sends input to a [`Screen`]'s emulator. Cloned freely; sending after the screen ends does
/// nothing.
#[derive(Debug, Clone)]
pub struct ScreenSender(UnboundedSender<ScreenInput>);

impl ScreenSender {
    pub fn send(&self, input: ScreenInput) {
        let _ = self.0.unbounded_send(input);
    }
}

impl Screen {
    /// A screen whose frames arrive on `frames` and whose input is read from `input`.
    pub(super) fn new(
        frames: watch::Receiver<Latest>,
        input: UnboundedSender<ScreenInput>,
        display: (u32, u32),
    ) -> Self {
        Self {
            frames,
            input: ScreenSender(input),
            display,
        }
    }

    /// The newest frame once there is one newer than the last returned, skipping any in between.
    /// None once the screen has ended.
    pub async fn next(&mut self) -> Option<Result<Arc<ScreenFrame>, Error>> {
        loop {
            self.frames.changed().await.ok()?;
            let latest = self.frames.borrow_and_update().clone();
            match latest {
                Some(Ok(frame)) => return Some(Ok(frame)),
                Some(Err(e)) => return Some(Err(e.into())),
                None => {}
            }
        }
    }

    /// The screen's size in pixels as the device was made.
    #[must_use]
    pub fn display(&self) -> (u32, u32) {
        self.display
    }

    #[must_use]
    pub fn sender(&self) -> ScreenSender {
        self.input.clone()
    }
}

/// Where an emulator's gRPC server listens, and how to reach the device's screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Endpoint {
    pub(super) port: u16,
    pub(super) token: String,
    /// The screen's size in pixels, which taps are placed on.
    pub(super) lcd: (u32, u32),
    /// Whether the device has a keyboard, without which the emulator drops the keys it is sent.
    pub(super) keyboard: bool,
}

/// The folders emulators write their discovery files to, on any platform.
fn discovery_dirs() -> Vec<PathBuf> {
    let home = crate::system::fs::home_dir();
    let mut dirs = Vec::new();
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        dirs.push(PathBuf::from(runtime).join("avd").join("running"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(
            PathBuf::from(local)
                .join("Temp")
                .join("avd")
                .join("running"),
        );
    }
    if let Some(home) = home {
        dirs.push(home.join("Library/Caches/TemporaryItems/avd/running"));
        dirs.push(home.join(".android").join("avd").join("running"));
    }
    dirs
}

/// The gRPC endpoint of the emulator whose console is on `console`, from the newest discovery
/// file that names it.
fn endpoint(console: u16) -> Option<Endpoint> {
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = discovery_dirs()
        .iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flatten()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".ini"))
        .map(|entry| {
            let modified = entry.metadata().and_then(|m| m.modified()).ok();
            (modified.unwrap_or(std::time::UNIX_EPOCH), entry.path())
        })
        .collect();
    found.sort();
    found
        .iter()
        .rev()
        .find_map(|(_, path)| parse_discovery(&std::fs::read_to_string(path).ok()?, console))
}

/// The endpoint a discovery file describes, when it is the emulator on `console`.
pub(super) fn parse_discovery(text: &str, console: u16) -> Option<Endpoint> {
    let values = ini(text);
    if values.get("port.serial")?.parse::<u16>().ok()? != console {
        return None;
    }
    let config = values
        .get("avd.dir")
        .and_then(|dir| std::fs::read_to_string(Path::new(dir).join("config.ini")).ok())
        .map(|config| ini(&config))
        .unwrap_or_default();
    let keyboard = config.get("hw.keyboard").is_none_or(|on| on == "yes");
    let lcd = Some(&config)
        .and_then(|config| {
            let size = |key: &str| config.get(key)?.parse::<u32>().ok();
            Some((size("hw.lcd.width")?, size("hw.lcd.height")?))
        })
        .unwrap_or((1080, 2400));
    Some(Endpoint {
        port: values.get("grpc.port")?.parse().ok()?,
        token: values.get("grpc.token").cloned().unwrap_or_default(),
        lcd,
        keyboard,
    })
}

/// The event the emulator is sent for `input`, with points placed on a screen of `lcd` pixels.
fn event(input: ScreenInput, lcd: (u32, u32)) -> Option<InputEvent> {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        reason = "a point on a screen a few thousand pixels across"
    )]
    let place = |fraction: f32, pixels: u32| (fraction.clamp(0., 1.) * (pixels - 1) as f32) as i32;
    let event = match input {
        ScreenInput::Touch { x, y, down } => Event::Mouse(MouseEvent {
            x: place(x, lcd.0),
            y: place(y, lcd.1),
            buttons: i32::from(down),
        }),
        ScreenInput::Key(key) => Event::Key(KeyboardEvent {
            event_type: KEYPRESS,
            key,
            text: String::new(),
        }),
        ScreenInput::Text(text) => Event::Key(KeyboardEvent {
            event_type: KEYPRESS,
            key: String::new(),
            text,
        }),
        ScreenInput::Wheel { .. } | ScreenInput::Fit { .. } => return None,
    };
    Some(InputEvent { event: Some(event) })
}

/// The adb arguments that press a key or type text on a device whose keyboard is off: the
/// Android key code for each key the panel sends, and the text quoted for the device's shell,
/// where `input text` reads a space as `%s`.
pub(super) fn adb_args(input: &ScreenInput) -> Option<Vec<String>> {
    let input_args = |rest: [String; 2]| {
        let mut args = vec!["shell".to_string(), "input".to_string()];
        args.extend(rest);
        args
    };
    match input {
        ScreenInput::Key(key) => {
            let code = match key.as_str() {
                "GoBack" => "KEYCODE_BACK",
                "GoHome" => "KEYCODE_HOME",
                "AppSwitch" => "KEYCODE_APP_SWITCH",
                "Power" => "KEYCODE_POWER",
                "AudioVolumeUp" => "KEYCODE_VOLUME_UP",
                "AudioVolumeDown" => "KEYCODE_VOLUME_DOWN",
                "Enter" => "KEYCODE_ENTER",
                "Backspace" => "KEYCODE_DEL",
                "Delete" => "KEYCODE_FORWARD_DEL",
                "Tab" => "KEYCODE_TAB",
                "Escape" => "KEYCODE_ESCAPE",
                "ArrowLeft" => "KEYCODE_DPAD_LEFT",
                "ArrowRight" => "KEYCODE_DPAD_RIGHT",
                "ArrowUp" => "KEYCODE_DPAD_UP",
                "ArrowDown" => "KEYCODE_DPAD_DOWN",
                "Home" => "KEYCODE_MOVE_HOME",
                "End" => "KEYCODE_MOVE_END",
                "PageUp" => "KEYCODE_PAGE_UP",
                "PageDown" => "KEYCODE_PAGE_DOWN",
                _ => return None,
            };
            Some(input_args(["keyevent".to_string(), code.to_string()]))
        }
        ScreenInput::Text(text) => {
            let text = text.replace(' ', "%s").replace('\'', "'\\''");
            Some(input_args(["text".to_string(), format!("'{text}'")]))
        }
        _ => None,
    }
}

/// A frame from the emulator's picture, turned from RGBA into BGRA. None for the empty picture
/// of a screen that is off.
fn frame(image: Image) -> Option<ScreenFrame> {
    let format = image.format?;
    let mut bgra = image.image;
    if format.width == 0 || bgra.len() != (format.width * format.height * 4) as usize {
        return None;
    }
    for pixel in bgra.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    Some(ScreenFrame {
        width: format.width,
        height: format.height,
        bgra,
    })
}

/// Streams the screen at `endpoint` into `latest`, and sends what comes in on `inputs`, until
/// the [`Screen`] is dropped or the emulator closes.
async fn stream(
    endpoint: Endpoint,
    latest: watch::Sender<Latest>,
    mut inputs: UnboundedReceiver<ScreenInput>,
    adb: Option<Adb>,
) {
    let mut client = match Client::connect(endpoint.port, endpoint.token.clone()).await {
        Ok(client) => client,
        Err(e) => {
            latest.send_replace(Some(Err(format!(
                "Couldn't reach the emulator's screen ({e}). Stop the emulator and start it again."
            ))));
            return;
        }
    };
    let (fit_tx, mut fit) = watch::channel(None::<(u32, u32)>);
    let (events_tx, events) = unbounded();
    let lcd = endpoint.lcd;
    tokio::spawn(async move {
        while let Some(input) = inputs.next().await {
            match input {
                ScreenInput::Fit { width, height } => {
                    fit_tx.send_replace(Some((width, height)));
                }
                ScreenInput::Wheel { x, y, dy } => {
                    let to = (y + dy * WHEEL_DRAG).clamp(0., 1.);
                    let touch = |y: f32, down: bool| {
                        if let Some(touch) = event(ScreenInput::Touch { x, y, down }, lcd) {
                            let _ = events_tx.unbounded_send(touch);
                        }
                    };
                    for step in 0..=WHEEL_STEPS {
                        let at = y + (to - y) * f32::from(step) / f32::from(WHEEL_STEPS);
                        touch(at, true);
                        tokio::time::sleep(WHEEL_STEP).await;
                    }
                    tokio::time::sleep(WHEEL_STEP * 4).await;
                    touch(to, true);
                    touch(to, false);
                }
                // Without a keyboard the emulator drops keys, so adb types them instead.
                ScreenInput::Key(_) | ScreenInput::Text(_) if !endpoint.keyboard => {
                    let Some(args) = adb_args(&input) else {
                        continue;
                    };
                    let Some(adb) = adb.clone() else {
                        continue;
                    };
                    let typed = tokio::task::spawn_blocking(move || adb(&args)).await;
                    if let Ok(Err(e)) = typed {
                        log::warn!(error:% = e; "adb did not type a key: {{error}}");
                    }
                }
                input => {
                    if let Some(event) = event(input, lcd) {
                        let _ = events_tx.unbounded_send(event);
                    }
                }
            }
        }
    });
    let mut sending = client.clone();
    tokio::spawn(async move {
        if let Err(e) = sending.input(events).await {
            log::warn!(error:% = e; "the emulator stopped taking input: {{error}}");
        }
    });
    loop {
        let size = loop {
            if let Some(size) = *fit.borrow_and_update() {
                break size;
            }
            if fit.changed().await.is_err() {
                return;
            }
        };
        let format = ImageFormat {
            format: RGBA8888,
            width: size.0,
            height: size.1,
        };
        let mut frames = match client.screenshots(format).await {
            Ok(frames) => frames,
            Err(e) => {
                latest.send_replace(Some(Err(format!(
                    "The emulator didn't send its screen: {}. Stop the emulator and start it \
                     again.",
                    e.message()
                ))));
                return;
            }
        };
        loop {
            tokio::select! {
                changed = fit.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    break;
                }
                () = latest.closed() => return,
                message = frames.message() => match message {
                    Ok(Some(image)) => {
                        if let Some(frame) = frame(image) {
                            latest.send_replace(Some(Ok(Arc::new(frame))));
                        }
                    }
                    // The emulator closed.
                    Ok(None) => return,
                    Err(e) => {
                        latest.send_replace(Some(Err(format!(
                            "The emulator's screen stopped: {}.",
                            e.message()
                        ))));
                        return;
                    }
                },
            }
        }
    }
}

impl Server {
    /// The screen of the device `serial`, shown until the returned [`Screen`] is dropped. An
    /// emulator's is streamed, its frames starting once its size is sent with
    /// [`ScreenInput::Fit`]; a phone's is a picture taken about once a second.
    ///
    /// # Errors
    ///
    /// Returns an error when an emulator was started without gRPC, so it has only its own
    /// window, or a phone has no SDK to reach it with.
    pub fn android_screen(&self, project: &Path, serial: &str) -> Result<Screen, Error> {
        let Some(console) = emulator_port(serial) else {
            return Ok(super::mirror::start(self.android_sdk(project)?, serial)?);
        };
        let endpoint = endpoint(console).ok_or(
            "This emulator was started without its screen stream, so it has only its own \
             window. Stop it and start it here to see it in this panel.",
        )?;
        let adb: Option<Adb> = self.android_sdk(project).ok().and_then(|sdk| {
            let program = sdk.adb.clone()?;
            let serial = serial.to_string();
            Some(Arc::new(move |args: &[String]| {
                let mut cmd = tool_command(&sdk, &program);
                cmd.args(["-s", &serial]).args(args);
                tool::output(cmd, ADB_LIMIT).map(|_| ())
            }) as Adb)
        });
        let display = endpoint.lcd;
        let (latest_tx, latest) = watch::channel(None);
        let (input, inputs) = unbounded();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        std::thread::Builder::new()
            .name(format!("screen {serial}"))
            .spawn(move || runtime.block_on(stream(endpoint, latest_tx, inputs, adb)))?;
        Ok(Screen {
            display,
            frames: latest,
            input: ScreenSender(input),
        })
    }
}
