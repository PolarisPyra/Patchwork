//! One page in the headless browser, driven over the `DevTools` protocol: the app's commands
//! turned into protocol calls, and the protocol's replies and events turned into frames and
//! events for the app.
//!
//! The page is a target of its own, attached with a flat session, so every page call carries its
//! `sessionId`. Its pictures come from a screencast, one JPEG for each frame the page draws,
//! each acknowledged so the next is sent. The JPEGs are decoded on a thread of their own, newest
//! first, so input never waits behind a picture.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::mpsc::{Receiver, Sender};

use base64::Engine as _;
use futures::channel::mpsc::UnboundedSender;
use serde_json::{Value, json};
use tungstenite::WebSocket;
use tungstenite::stream::MaybeTlsStream;
use zune_jpeg::JpegDecoder;
use zune_jpeg::zune_core::bytestream::ZCursor;
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::DecoderOptions;

use super::keys::{key_events, modifier_bits};
use super::types::{
    BrowserCommand, BrowserEvent, BrowserFrame, BrowserMessage, MouseAction, PointerButton,
};
use crate::Error;

pub(super) type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

/// The screencast's JPEG quality: sharp text without frames so large they lag.
const QUALITY: u32 = 85;

/// What a call's reply is for.
#[derive(Debug, Clone, Copy)]
enum Reply {
    Created,
    Attached,
    History,
    Picked,
}

/// Outlines the element at `__X`, `__Y` with a box the pointer passes through.
const HIGHLIGHT: &str = r"(() => {
  let o = document.getElementById('__patchwork_pick');
  const el = document.elementFromPoint(__X, __Y);
  if (!el) { if (o) o.remove(); return; }
  if (!o) {
    o = document.createElement('div');
    o.id = '__patchwork_pick';
    o.style.cssText = 'position:fixed;pointer-events:none;z-index:2147483647;' +
      'border:2px solid #3b82f6;background:rgba(59,130,246,.12);border-radius:2px;box-sizing:border-box';
    document.documentElement.appendChild(o);
  }
  const r = el.getBoundingClientRect();
  Object.assign(o.style, { left: r.left + 'px', top: r.top + 'px', width: r.width + 'px', height: r.height + 'px' });
})()";

/// Clears the outline.
const UNHIGHLIGHT: &str =
    r"(() => { const o = document.getElementById('__patchwork_pick'); if (o) o.remove(); })()";

/// The element at `__X`, `__Y`: a selector for it, its HTML cut short, and the styles that
/// shape it.
const PICK: &str = r"(() => {
  const o = document.getElementById('__patchwork_pick'); if (o) o.remove();
  const el = document.elementFromPoint(__X, __Y);
  if (!el) return null;
  const part = (e) => {
    if (e.id) return '#' + CSS.escape(e.id);
    let p = e.tagName.toLowerCase();
    p += [...e.classList].slice(0, 3).map(c => '.' + CSS.escape(c)).join('');
    const parent = e.parentElement;
    if (parent) {
      const same = [...parent.children].filter(c => c.tagName === e.tagName);
      if (same.length > 1) p += ':nth-of-type(' + (same.indexOf(e) + 1) + ')';
    }
    return p;
  };
  const parts = [];
  for (let e = el; e && e.nodeType === 1 && e !== document.documentElement && parts.length < 6; e = e.parentElement) {
    parts.unshift(part(e));
    if (e.id) break;
  }
  const cs = getComputedStyle(el);
  const keys = ['display', 'position', 'width', 'height', 'margin', 'padding', 'color',
    'background-color', 'font-family', 'font-size', 'font-weight', 'line-height', 'border',
    'border-radius', 'gap', 'flex-direction', 'justify-content', 'align-items', 'opacity'];
  const styles = keys.map(k => k + ': ' + cs.getPropertyValue(k) + ';').join('
');
  let html = el.outerHTML;
  if (html.length > 4000) html = html.slice(0, 4000) + '
<!-- cut short -->';
  return { selector: parts.join(' > '), html, styles };
})()";

/// The page's state between protocol messages.
#[derive(Debug)]
pub(super) struct Page {
    socket: Socket,
    out: UnboundedSender<BrowserMessage>,
    /// Each screencast frame's JPEG, as base64, for [`decode_frames`].
    frames: Sender<String>,
    next_id: u64,
    replies: HashMap<u64, Reply>,
    target: Option<String>,
    session: Option<String>,
    main_frame: Option<String>,
    /// The entry the page is on, and every entry's id.
    history: (usize, Vec<i64>),
    /// The page's size in logical pixels and its scale.
    size: (u32, u32, f32),
    /// The mouse buttons held, as the protocol's bits.
    buttons: u32,
    /// Commands sent before the page was attached, run once it is.
    waiting: Vec<BrowserCommand>,
}

impl Page {
    /// Asks the browser for a new page, `size` logical pixels across and down at device pixels
    /// per one; the rest follows from its replies.
    ///
    /// # Errors
    ///
    /// Returns an error when the browser cannot be written to.
    pub fn open(
        socket: Socket,
        out: UnboundedSender<BrowserMessage>,
        frames: Sender<String>,
        size: (u32, u32, f32),
    ) -> Result<Self, Error> {
        let mut page = Self {
            socket,
            out,
            frames,
            next_id: 0,
            replies: HashMap::new(),
            target: None,
            session: None,
            main_frame: None,
            history: (0, Vec::new()),
            size,
            buttons: 0,
            waiting: Vec::new(),
        };
        page.call(
            "Target.setDiscoverTargets",
            json!({ "discover": true }),
            None,
        )?;
        page.call(
            "Target.createTarget",
            json!({ "url": "about:blank" }),
            Some(Reply::Created),
        )?;
        Ok(page)
    }

    pub fn socket(&mut self) -> &mut Socket {
        &mut self.socket
    }

    /// Runs a command from the app, or keeps it until the page is attached.
    ///
    /// # Errors
    ///
    /// Returns an error when the browser cannot be written to.
    pub fn command(&mut self, command: BrowserCommand) -> Result<(), Error> {
        if self.session.is_none() {
            if let BrowserCommand::Resize {
                width,
                height,
                scale,
            } = command
            {
                self.size = (width, height, scale);
            } else {
                self.waiting.push(command);
            }
            return Ok(());
        }
        match command {
            BrowserCommand::Open { url } => {
                self.page_call("Page.navigate", json!({ "url": address(&url) }))
            }
            BrowserCommand::Resize {
                width,
                height,
                scale,
            } => {
                self.size = (width, height, scale);
                self.apply_size()
            }
            BrowserCommand::Mouse {
                x,
                y,
                action,
                button,
                clicks,
                modifiers,
            } => {
                let bit = match button {
                    PointerButton::Left => 1,
                    PointerButton::Right => 2,
                    PointerButton::Middle => 4,
                };
                let kind = match action {
                    MouseAction::Move => "mouseMoved",
                    MouseAction::Down => {
                        self.buttons |= bit;
                        "mousePressed"
                    }
                    MouseAction::Up => {
                        self.buttons &= !bit;
                        "mouseReleased"
                    }
                    MouseAction::Leave => return Ok(()),
                };
                let button = if action == MouseAction::Move {
                    "none"
                } else {
                    match button {
                        PointerButton::Left => "left",
                        PointerButton::Middle => "middle",
                        PointerButton::Right => "right",
                    }
                };
                let event = json!({
                    "type": kind,
                    "x": x,
                    "y": y,
                    "button": button,
                    "buttons": self.buttons,
                    "clickCount": clicks,
                    "modifiers": modifier_bits(modifiers),
                });
                self.page_call("Input.dispatchMouseEvent", event)
            }
            BrowserCommand::Wheel {
                x,
                y,
                dx,
                dy,
                modifiers,
            } => {
                let event = json!({
                    "type": "mouseWheel",
                    "x": x,
                    "y": y,
                    "deltaX": dx,
                    "deltaY": dy,
                    "modifiers": modifier_bits(modifiers),
                });
                self.page_call("Input.dispatchMouseEvent", event)
            }
            BrowserCommand::Key {
                key,
                text,
                modifiers,
            } => key_events(&key, text.as_deref(), modifiers)
                .into_iter()
                .try_for_each(|event| self.page_call("Input.dispatchKeyEvent", event)),
            BrowserCommand::Text { text } => {
                self.page_call("Input.insertText", json!({ "text": text }))
            }
            BrowserCommand::Theme { dark } => self.page_call(
                "Emulation.setEmulatedMedia",
                json!({ "features": [{
                    "name": "prefers-color-scheme",
                    "value": if dark { "dark" } else { "light" },
                }] }),
            ),
            BrowserCommand::Highlight { x, y } => self.page_call(
                "Runtime.evaluate",
                json!({ "expression": at(HIGHLIGHT, x, y) }),
            ),
            BrowserCommand::Pick { x, y } => {
                let session = self.session.clone();
                self.send_call(
                    "Runtime.evaluate",
                    json!({ "expression": at(PICK, x, y), "returnByValue": true }),
                    session,
                    Some(Reply::Picked),
                )
            }
            BrowserCommand::Unhighlight => {
                self.page_call("Runtime.evaluate", json!({ "expression": UNHIGHLIGHT }))
            }
            BrowserCommand::Back => self.go(-1),
            BrowserCommand::Forward => self.go(1),
            BrowserCommand::Reload => self.page_call("Page.reload", json!({})),
            BrowserCommand::Stop => self.page_call("Page.stopLoading", json!({})),
        }
    }

    /// Asks the browser to close, which ends it.
    pub fn close(&mut self) {
        let _ = self.call("Browser.close", json!({}), None);
    }

    /// Handles one message from the browser.
    ///
    /// # Errors
    ///
    /// Returns an error when the browser cannot be written to.
    pub fn message(&mut self, text: &str) -> Result<(), Error> {
        let Ok(message) = serde_json::from_str::<Value>(text) else {
            return Ok(());
        };
        if let Some(id) = message["id"].as_u64() {
            if let Some(error) = message.get("error") {
                log::warn!(error:% = error; "browser call failed: {error}");
            }
            return match self.replies.remove(&id) {
                Some(reply) => self.reply(reply, &message["result"]),
                None => Ok(()),
            };
        }
        let params = &message["params"];
        match message["method"].as_str().unwrap_or_default() {
            "Target.targetInfoChanged" => self.target_changed(&params["targetInfo"]),
            method if message["sessionId"].as_str() == self.session.as_deref() => {
                self.page_event(method, params)
            }
            _ => Ok(()),
        }
    }

    fn reply(&mut self, reply: Reply, result: &Value) -> Result<(), Error> {
        match reply {
            Reply::Created => {
                let target = result["targetId"].as_str().unwrap_or_default().to_owned();
                self.call(
                    "Target.attachToTarget",
                    json!({ "targetId": target, "flatten": true }),
                    Some(Reply::Attached),
                )?;
                self.target = Some(target);
                Ok(())
            }
            Reply::Attached => {
                self.session = result["sessionId"].as_str().map(str::to_owned);
                self.page_call("Page.enable", json!({}))?;
                // For the errors the page meets, which the app shows.
                self.page_call("Runtime.enable", json!({}))?;
                self.page_call("Log.enable", json!({}))?;
                self.page_call(
                    "Emulation.setFocusEmulationEnabled",
                    json!({ "enabled": true }),
                )?;
                self.apply_size()?;
                // Frames follow the page's size, so the screencast never needs restarting.
                self.page_call(
                    "Page.startScreencast",
                    json!({ "format": "jpeg", "quality": QUALITY }),
                )?;
                std::mem::take(&mut self.waiting)
                    .into_iter()
                    .try_for_each(|command| self.command(command))
            }
            Reply::History => {
                let entries = result["entries"]
                    .as_array()
                    .map_or_else(Vec::new, |entries| {
                        entries.iter().filter_map(|e| e["id"].as_i64()).collect()
                    });
                let current = result["currentIndex"]
                    .as_u64()
                    .and_then(|i| usize::try_from(i).ok())
                    .unwrap_or_default();
                self.history = (current, entries);
                self.loading(false);
                Ok(())
            }
            Reply::Picked => {
                let value = &result["result"]["value"];
                let text = |key: &str| value[key].as_str().unwrap_or_default().to_owned();
                if value.is_object() {
                    self.send(BrowserEvent::Picked {
                        selector: text("selector"),
                        html: text("html"),
                        styles: text("styles"),
                    });
                }
                Ok(())
            }
        }
    }

    fn page_event(&mut self, method: &str, params: &Value) -> Result<(), Error> {
        match method {
            "Page.screencastFrame" => {
                self.page_call(
                    "Page.screencastFrameAck",
                    json!({ "sessionId": params["sessionId"] }),
                )?;
                if let Some(data) = params["data"].as_str() {
                    let _ = self.frames.send(data.to_owned());
                }
            }
            "Page.frameNavigated" if params["frame"]["parentId"].is_null() => {
                self.main_frame = params["frame"]["id"].as_str().map(str::to_owned);
                self.address(&params["frame"]["url"]);
            }
            "Page.navigatedWithinDocument" if self.is_main(&params["frameId"]) => {
                self.address(&params["url"]);
                self.call_history()?;
            }
            "Page.frameStartedLoading" if self.is_main(&params["frameId"]) => self.loading(true),
            "Page.frameStoppedLoading" if self.is_main(&params["frameId"]) => {
                self.call_history()?;
            }
            "Runtime.exceptionThrown" | "Runtime.consoleAPICalled" | "Log.entryAdded" => {
                if let Some(message) = page_error(method, params) {
                    self.send(BrowserEvent::PageError { message });
                }
            }
            // A page's alert or confirm would hold it until answered, with nobody to see it.
            "Page.javascriptDialogOpening" => {
                self.page_call("Page.handleJavaScriptDialog", json!({ "accept": true }))?;
            }
            _ => {}
        }
        Ok(())
    }

    /// The page's title, or a new window it opened, which loads in its place instead.
    fn target_changed(&mut self, info: &Value) -> Result<(), Error> {
        let id = info["targetId"].as_str();
        if id.is_some() && id == self.target.as_deref() {
            if let Some(title) = info["title"].as_str() {
                self.send(BrowserEvent::Title {
                    title: title.to_owned(),
                });
            }
            return Ok(());
        }
        let url = info["url"].as_str().unwrap_or_default();
        if info["openerId"].as_str() == self.target.as_deref()
            && info["type"] == "page"
            && !url.is_empty()
            && url != "about:blank"
        {
            let url = url.to_owned();
            self.call("Target.closeTarget", json!({ "targetId": id }), None)?;
            self.page_call("Page.navigate", json!({ "url": url }))?;
        }
        Ok(())
    }

    fn is_main(&self, frame: &Value) -> bool {
        self.main_frame.is_none() || frame.as_str() == self.main_frame.as_deref()
    }

    fn address(&self, url: &Value) {
        if let Some(url) = url.as_str() {
            self.send(BrowserEvent::Address {
                url: url.to_owned(),
            });
        }
    }

    fn loading(&self, loading: bool) {
        let (current, entries) = &self.history;
        self.send(BrowserEvent::Loading {
            loading,
            back: *current > 0,
            forward: current + 1 < entries.len(),
        });
    }

    fn send(&self, event: BrowserEvent) {
        let _ = self.out.unbounded_send(BrowserMessage::Event(event));
    }

    fn go(&mut self, step: isize) -> Result<(), Error> {
        let (current, entries) = &self.history;
        let Some(entry) = current
            .checked_add_signed(step)
            .and_then(|i| entries.get(i))
        else {
            return Ok(());
        };
        let entry = *entry;
        self.page_call("Page.navigateToHistoryEntry", json!({ "entryId": entry }))
    }

    fn call_history(&mut self) -> Result<(), Error> {
        let session = self.session.clone();
        self.send_call(
            "Page.getNavigationHistory",
            json!({}),
            session,
            Some(Reply::History),
        )
    }

    /// Sizes the page.
    fn apply_size(&mut self) -> Result<(), Error> {
        let (width, height, scale) = self.size;
        self.page_call(
            "Emulation.setDeviceMetricsOverride",
            json!({
                "width": width,
                "height": height,
                "deviceScaleFactor": scale,
                "mobile": false,
            }),
        )
    }

    fn page_call(&mut self, method: &str, params: Value) -> Result<(), Error> {
        let session = self.session.clone();
        self.send_call(method, params, session, None)
    }

    fn call(&mut self, method: &str, params: Value, reply: Option<Reply>) -> Result<(), Error> {
        self.send_call(method, params, None, reply)
    }

    fn send_call(
        &mut self,
        method: &str,
        params: Value,
        session: Option<String>,
        reply: Option<Reply>,
    ) -> Result<(), Error> {
        self.next_id += 1;
        let mut message = json!({ "id": self.next_id, "method": method });
        message["params"] = params;
        if let Some(session) = session {
            message["sessionId"] = json!(session);
        }
        if let Some(reply) = reply {
            self.replies.insert(self.next_id, reply);
        }
        self.socket
            .send(tungstenite::Message::text(message.to_string()))
            .map_err(|e| Error::Message(format!("The browser stopped answering: {e}")))
    }
}

/// `script` with its point filled in.
fn at(script: &str, x: f32, y: f32) -> String {
    script
        .replace("__X", &format!("{x:.1}"))
        .replace("__Y", &format!("{y:.1}"))
}

/// Decodes the page's frames and sends them to the app, skipping any a newer one has already
/// replaced. Ends when the page is gone or the app stops listening.
pub(super) fn decode_frames(frames: &Receiver<String>, out: &UnboundedSender<BrowserMessage>) {
    while let Ok(mut data) = frames.recv() {
        while let Ok(newer) = frames.try_recv() {
            data = newer;
        }
        if let Some(frame) = decode(&data)
            && out.unbounded_send(BrowserMessage::Frame(frame)).is_err()
        {
            return;
        }
    }
}

/// A screencast frame's JPEG as blue, green, red and alpha pixels.
fn decode(data: &str) -> Option<BrowserFrame> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .ok()?;
    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::BGRA);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(&bytes), options);
    let bgra = decoder.decode().ok()?;
    let (width, height) = decoder.dimensions()?;
    Some(BrowserFrame {
        width: u32::try_from(width).ok()?,
        height: u32::try_from(height).ok()?,
        bgra,
    })
}

/// What was typed in an address bar as an address the browser loads: a scheme added when it has
/// none, plain `http` for this computer's own servers.
#[must_use]
pub fn address(typed: &str) -> String {
    let typed = typed.trim();
    if typed.contains("://") || typed.starts_with("about:") || typed.starts_with("data:") {
        return typed.to_owned();
    }
    if typed.starts_with('/') {
        return format!("file://{typed}");
    }
    let local = ["localhost", "127.0.0.1", "0.0.0.0", "[::1]"]
        .iter()
        .any(|host| typed.starts_with(host));
    format!("{}://{typed}", if local { "http" } else { "https" })
}

/// The error a page's protocol event `method` reports, with where it happened, if it is one.
fn page_error(method: &str, params: &Value) -> Option<String> {
    let at = |url: &Value, line: &Value| {
        let file = url
            .as_str()
            .and_then(|u| u.rsplit('/').next())
            .unwrap_or_default();
        match line.as_u64() {
            Some(line) if !file.is_empty() => format!(" ({file}:{})", line + 1),
            _ => String::new(),
        }
    };
    match method {
        "Runtime.exceptionThrown" => {
            let details = &params["exceptionDetails"];
            let text = details["exception"]["description"]
                .as_str()
                .or_else(|| details["text"].as_str())?;
            // The stack follows the first line; the place is given once.
            let first = text.lines().next().unwrap_or(text);
            Some(format!(
                "{first}{}",
                at(&details["url"], &details["lineNumber"])
            ))
        }
        "Runtime.consoleAPICalled" if params["type"] == "error" => {
            let parts: Vec<String> = params["args"]
                .as_array()?
                .iter()
                .map(|arg| match &arg["value"] {
                    Value::String(s) => s.clone(),
                    Value::Null => arg["description"].as_str().unwrap_or_default().to_owned(),
                    other => other.to_string(),
                })
                .collect();
            Some(parts.join(" ")).filter(|s| !s.trim().is_empty())
        }
        "Log.entryAdded" if params["entry"]["level"] == "error" => {
            let entry = &params["entry"];
            let text = entry["text"].as_str()?;
            Some(format!("{text}{}", at(&entry["url"], &entry["lineNumber"])))
        }
        _ => None,
    }
}
