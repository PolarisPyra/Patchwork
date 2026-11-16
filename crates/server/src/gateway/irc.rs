//! IRC: one connection to a server, as a nickname that answers private messages and, in the
//! channels it joins, lines addressed to it ("patchwork: …"). IRC has no buttons or edits.

// Rust guideline compliant 2026-10-09

use std::sync::atomic::Ordering;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use super::net::chunks;
use super::platform::Platform;
use super::stream::{Lines, host_port, open};
use super::types::{Button, Connector, Incoming, Listener};
use crate::Error;

/// How long a read waits before the connection checks whether to stop or send.
const READ_WAIT: Duration = Duration::from_millis(250);

/// How long the server may stay quiet before Patchwork pings it, and again before giving up.
const QUIET: Duration = Duration::from_secs(150);

/// The pause between lines Patchwork sends, so the server does not drop it for flooding.
const SEND_GAP: Duration = Duration::from_millis(300);

/// The longest line Patchwork sends, in characters, which keeps it under IRC's 512 bytes.
const LINE_LIMIT: usize = 350;

/// The port of IRC over TLS, and of IRC without it.
const TLS_PORT: u16 = 6697;
const PLAIN_PORT: u16 = 6667;

/// What an IRC connection is made from, as Settings holds it.
pub(crate) struct IrcSetup {
    /// `host:port`, after `ircs://` or `irc://`; TLS unless it says `irc://`.
    pub server: String,
    pub nick: String,
    /// Channels to join, separated by commas or spaces.
    pub channels: String,
    /// The server password; empty for none.
    pub password: String,
    /// The password NickServ identifies the nickname with; empty for none.
    pub nickserv: String,
}

pub(crate) struct Irc {
    host: String,
    port: u16,
    tls: bool,
    nick: String,
    channels: Vec<String>,
    password: String,
    nickserv: String,
    /// Lines for the running connection to send.
    outbox: Mutex<Option<Sender<String>>>,
}

impl Irc {
    /// An IRC connection made from `setup`.
    ///
    /// # Errors
    ///
    /// Returns an error when the server's port is not a number.
    pub(crate) fn new(setup: IrcSetup) -> Result<Self, Error> {
        let server = setup.server.trim();
        let (tls, address) = if let Some(rest) = server.strip_prefix("ircs://") {
            (true, rest)
        } else if let Some(rest) = server.strip_prefix("irc://") {
            (false, rest)
        } else {
            (true, server)
        };
        let port = if tls { TLS_PORT } else { PLAIN_PORT };
        let (host, port) = host_port(address.trim_end_matches('/'), port)?;
        let channels = setup
            .channels
            .split([',', ' '])
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .map(|c| {
                if c.starts_with(['#', '&', '+', '!']) {
                    c.to_string()
                } else {
                    format!("#{c}")
                }
            })
            .collect();
        Ok(Self {
            host,
            port,
            tls,
            nick: setup.nick,
            channels,
            password: setup.password,
            nickserv: setup.nickserv,
            outbox: Mutex::new(None),
        })
    }

    fn queue(&self, line: String) -> Result<(), Error> {
        let outbox = self.outbox.lock().unwrap_or_else(PoisonError::into_inner);
        outbox
            .as_ref()
            .ok_or("IRC is not connected")?
            .send(line)
            .map_err(|_closed| "IRC is not connected".into())
    }
}

impl Connector for Irc {
    fn listen(&self, to: &Listener<'_>) -> Result<(), Error> {
        let stream = open("IRC", &self.host, self.port, self.tls, READ_WAIT)?;
        let mut lines = Lines::new("The IRC server", stream);
        let (sender, outgoing) = channel();
        *self.outbox.lock().unwrap_or_else(PoisonError::into_inner) = Some(sender);
        if !self.password.is_empty() {
            lines.send(&format!("PASS {}", self.password))?;
        }
        let mut nick = self.nick.clone();
        let mut welcomed = false;
        lines.send(&format!("NICK {nick}"))?;
        lines.send(&format!("USER {nick} 0 * :Patchwork"))?;
        let mut heard = Instant::now();
        let mut pinged = false;
        loop {
            if to.stop.load(Ordering::Relaxed) {
                let _ = lines.send("QUIT :Patchwork stopped");
                return Ok(());
            }
            while let Ok(line) = outgoing.try_recv() {
                lines.send(&line)?;
                std::thread::sleep(SEND_GAP);
            }
            let Some(line) = lines.line()? else {
                if heard.elapsed() > QUIET * 2 {
                    return Err("The IRC server stopped answering".into());
                }
                if heard.elapsed() > QUIET && !pinged {
                    lines.send("PING :patchwork")?;
                    pinged = true;
                }
                continue;
            };
            heard = Instant::now();
            pinged = false;
            let message = parse(&line);
            match message.command {
                "PING" => lines.send(&format!("PONG :{}", message.last()))?,
                "001" => {
                    welcomed = true;
                    if let Some(name) = message.params.first() {
                        nick = (*name).to_string();
                    }
                    if !self.nickserv.is_empty() {
                        lines.send(&format!("PRIVMSG NickServ :IDENTIFY {}", self.nickserv))?;
                    }
                    for channel in &self.channels {
                        lines.send(&format!("JOIN {channel}"))?;
                    }
                    (to.connected)(nick.clone());
                }
                "433" if !welcomed => {
                    nick.push('_');
                    lines.send(&format!("NICK {nick}"))?;
                }
                "PRIVMSG" => {
                    if let Some(message) = incoming(&message, &nick) {
                        (to.inbox)(message);
                    }
                }
                "ERROR" => {
                    return Err(
                        format!("The IRC server ended the connection: {}", message.last()).into(),
                    );
                }
                _ => {}
            }
        }
    }

    fn send(&self, chat: &str, text: &str, _buttons: &[Button]) -> Result<Option<String>, Error> {
        for line in text.lines().map(str::trim_end).filter(|l| !l.is_empty()) {
            for piece in chunks(line, LINE_LIMIT) {
                self.queue(format!("PRIVMSG {chat} :{piece}"))?;
            }
        }
        Ok(None)
    }

    fn edit(&self, _chat: &str, _id: &str, _text: &str) -> Result<(), Error> {
        Err("IRC messages cannot be edited".into())
    }
}

/// A line from the server, split into its parts.
struct Message<'a> {
    /// Who sent it, as `nick!user@host`; empty when the server did.
    prefix: &'a str,
    command: &'a str,
    params: Vec<&'a str>,
}

impl Message<'_> {
    /// The last parameter, which holds a message's text.
    fn last(&self) -> &str {
        self.params.last().copied().unwrap_or_default()
    }
}

fn parse(line: &str) -> Message<'_> {
    let mut rest = line;
    // IRCv3 tags come first; nothing here reads them.
    if rest.starts_with('@') {
        rest = rest.split_once(' ').map_or("", |(_, r)| r);
    }
    let mut prefix = "";
    if let Some(after) = rest.strip_prefix(':') {
        (prefix, rest) = after.split_once(' ').unwrap_or((after, ""));
    }
    let (head, trailing) = match rest.split_once(" :") {
        Some((head, trailing)) => (head, Some(trailing)),
        None => (rest, None),
    };
    let mut words = head.split(' ').filter(|w| !w.is_empty());
    let command = words.next().unwrap_or_default();
    let mut params: Vec<_> = words.collect();
    params.extend(trailing);
    Message {
        prefix,
        command,
        params,
    }
}

/// The message in a `PRIVMSG` sent to Patchwork: a private one, or a channel line that starts
/// with its nickname.
fn incoming(message: &Message<'_>, nick: &str) -> Option<Incoming> {
    let [target, text] = message.params.as_slice() else {
        return None;
    };
    // CTCP requests, such as VERSION, are not messages.
    if text.starts_with('\u{1}') {
        return None;
    }
    let (sender, mask) = message
        .prefix
        .split_once('!')
        .unwrap_or((message.prefix, message.prefix));
    let (chat, text) = if target.eq_ignore_ascii_case(nick) {
        (sender.to_string(), text.trim())
    } else {
        let head = text.get(..nick.len())?;
        let rest = text.get(nick.len()..)?;
        if !head.eq_ignore_ascii_case(nick) || !rest.starts_with([':', ',']) {
            return None;
        }
        ((*target).to_string(), rest[1..].trim())
    };
    if text.is_empty() || sender.is_empty() {
        return None;
    }
    // The user and host are set by the server, so they say who is writing better than a nickname
    // anyone can take.
    Some(Incoming {
        platform: Platform::Irc,
        chat,
        user: mask.to_string(),
        name: sender.to_string(),
        text: text.to_string(),
        files: Vec::new(),
    })
}
