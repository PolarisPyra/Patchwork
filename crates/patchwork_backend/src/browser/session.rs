//! A page in a headless browser started for it: the protocol read and written on a thread of its
//! own, so nothing waits on the app's UI thread.
//!
//! The browser ends when the [`BrowserSession`] is dropped: the thread asks it to close, and it
//! is killed if it has not exited a moment later.

// Rust guideline compliant 2026-10-09

use std::collections::VecDeque;
use std::io::ErrorKind;
use std::sync::mpsc::{self, TryRecvError};
use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc::{UnboundedReceiver, unbounded};
use tungstenite::Message;
use tungstenite::stream::MaybeTlsStream;

use super::chrome::{reap, take_or_launch};
use super::page::{Page, Socket, decode_frames};
use super::types::{BrowserCommand, BrowserMessage};
use crate::Error;

/// How long the browser has to close after it is asked before it is killed. It writes its profile
/// and ends its child processes in well under this.
const EXIT_GRACE: Duration = Duration::from_secs(3);

/// How long a read waits for the browser before the thread looks for the app's commands again,
/// which bounds how late a click or key reaches the page.
const POLL: Duration = Duration::from_millis(8);

/// Sends commands to a [`BrowserSession`]'s page. Cloned freely; sending after the browser ends
/// does nothing.
#[derive(Debug, Clone)]
pub struct BrowserSender(mpsc::Sender<BrowserCommand>);

impl BrowserSender {
    pub fn send(&self, command: BrowserCommand) {
        let _ = self.0.send(command);
    }
}

/// A headless browser showing one page.
#[derive(Debug)]
pub struct BrowserSession {
    sender: BrowserSender,
    messages: UnboundedReceiver<BrowserMessage>,
    /// Messages read but not returned yet, events before the newest frame.
    pending: VecDeque<BrowserMessage>,
}

impl BrowserSession {
    /// Starts the browser on `url`, drawing a page `width` by `height` logical pixels at `scale`
    /// device pixels each. Blocks for the second or two the browser takes to start, so it runs
    /// off the UI thread.
    ///
    /// # Errors
    ///
    /// Returns an error when no browser is installed, or it cannot be started or reached.
    pub fn start(url: &str, width: u32, height: u32, scale: f32) -> Result<Self, Error> {
        let launched = take_or_launch()?;
        let socket = match connect(&launched.address) {
            Ok(socket) => socket,
            Err(e) => {
                reap(launched.child, &launched.profile, Duration::ZERO);
                return Err(e);
            }
        };

        let (messages_tx, messages) = unbounded();
        let (commands, queued) = mpsc::channel::<BrowserCommand>();
        let (frames_tx, frames) = mpsc::channel::<String>();
        let decoded = messages_tx.clone();
        std::thread::Builder::new()
            .name("browser-frames".into())
            .spawn(move || decode_frames(&frames, &decoded))?;
        std::thread::Builder::new()
            .name("browser-page".into())
            .spawn(move || {
                match Page::open(socket, messages_tx, frames_tx, (width, height, scale)) {
                    Ok(mut page) => {
                        if let Err(e) = pump(&mut page, &queued) {
                            log::warn!(error:% = e; "browser page ended: {{error}}");
                        }
                        page.close();
                    }
                    Err(e) => log::warn!(error:% = e; "browser page did not open: {{error}}"),
                }
                reap(launched.child, &launched.profile, EXIT_GRACE);
            })?;

        let sender = BrowserSender(commands);
        sender.send(BrowserCommand::Open {
            url: url.to_owned(),
        });
        Ok(Self {
            sender,
            messages,
            pending: VecDeque::new(),
        })
    }

    #[must_use]
    pub fn sender(&self) -> BrowserSender {
        self.sender.clone()
    }

    /// The next event, or the newest frame, skipping frames a newer one replaced. None once the
    /// browser has ended.
    pub async fn next(&mut self) -> Option<BrowserMessage> {
        if let Some(message) = self.pending.pop_front() {
            return Some(message);
        }
        let first = self.messages.next().await?;
        let mut frame = None;
        let mut take = |message| match message {
            BrowserMessage::Frame(f) => frame = Some(f),
            event @ BrowserMessage::Event(_) => self.pending.push_back(event),
        };
        take(first);
        while let Ok(message) = self.messages.try_recv() {
            take(message);
        }
        if let Some(frame) = frame {
            self.pending.push_back(BrowserMessage::Frame(frame));
        }
        self.pending.pop_front()
    }
}

/// Opens the browser's `DevTools` websocket, with reads that give up after [`POLL`].
fn connect(address: &str) -> Result<Socket, Error> {
    let (socket, _) = tungstenite::connect(address)
        .map_err(|e| Error::Message(format!("Could not reach the browser: {e}")))?;
    if let MaybeTlsStream::Plain(stream) = socket.get_ref() {
        stream.set_read_timeout(Some(POLL))?;
    }
    Ok(socket)
}

/// Runs the app's commands and the browser's messages until the session is dropped or the
/// browser goes away.
fn pump(page: &mut Page, commands: &mpsc::Receiver<BrowserCommand>) -> Result<(), Error> {
    loop {
        // A window being dragged larger sends a size for every frame; only the last one counts.
        let mut resize = None;
        loop {
            match commands.try_recv() {
                Ok(command @ BrowserCommand::Resize { .. }) => resize = Some(command),
                Ok(command) => page.command(command)?,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Ok(()),
            }
        }
        if let Some(command) = resize {
            page.command(command)?;
        }
        match page.socket().read() {
            Ok(Message::Text(text)) => page.message(text.as_str())?,
            Ok(Message::Close(_)) => return Ok(()),
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(e) => return Err(Error::Message(format!("The browser went away: {e}"))),
        }
    }
}
