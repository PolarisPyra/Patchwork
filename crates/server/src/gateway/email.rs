//! Email: Patchwork checks the inbox over IMAP for new mail and answers over SMTP, both over
//! TLS. A sender is taken at their word only when the receiving server's DMARC check passed, so
//! a forged `From` cannot reach a thread. Email has no buttons or edits.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::atomic::Ordering;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use mail_parser::{Message, MessageParser, MimeHeaders};

use super::hub::pause;
use super::platform::Platform;
use super::stream::{Lines, host_port, open};
use super::types::{Button, Connector, Incoming, Listener};
use crate::Error;

/// How often Patchwork checks the inbox.
const POLL: Duration = Duration::from_secs(20);

/// How long a read waits before the connection checks whether to stop.
const READ_WAIT: Duration = Duration::from_secs(1);

/// How long a server may take to answer a command.
const ANSWER_WAIT: Duration = Duration::from_secs(60);

/// IMAP over TLS, SMTP over TLS, and the SMTP ports that start in plain text and switch to TLS.
const IMAP_PORT: u16 = 993;
const SMTP_PORT: u16 = 465;
const STARTTLS_PORTS: [u16; 2] = [25, 587];

/// The length of a line of a base64 body.
const BASE64_LINE: usize = 76;

pub(crate) struct Email {
    address: String,
    imap: (String, u16),
    smtp: (String, u16),
    password: String,
    /// The next message number the inbox gives out, kept across reconnects so no mail is missed.
    next: Mutex<Option<u32>>,
    /// The subject and id of the last mail from each address, which replies answer.
    threads: Mutex<HashMap<String, (String, String)>>,
}

impl Email {
    /// An email connection that reads mail on `imap` and sends it on `smtp`.
    ///
    /// # Errors
    ///
    /// Returns an error when a server's port is not a number.
    pub(crate) fn new(
        address: String,
        imap: &str,
        smtp: &str,
        password: String,
    ) -> Result<Self, Error> {
        Ok(Self {
            address,
            imap: host_port(imap, IMAP_PORT)?,
            smtp: host_port(smtp, SMTP_PORT)?,
            password,
            next: Mutex::new(None),
            threads: Mutex::new(HashMap::new()),
        })
    }

    /// The message in a mail, when it is one a person sent and its sender is proven.
    fn incoming(&self, raw: &[u8]) -> Option<Incoming> {
        let mail = MessageParser::default().parse(raw)?;
        let from = mail.from()?.first()?;
        let address = from.address()?.trim().to_lowercase();
        if address == self.address.to_lowercase() || automatic(&mail) {
            return None;
        }
        if !proven(&mail) {
            log::warn!(address:% = address; "gateway email skipped mail from {{address}}: DMARC did not pass");
            return None;
        }
        let subject = mail.subject().unwrap_or_default().trim().to_string();
        if let Some(id) = mail.message_id() {
            self.threads
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(address.clone(), (subject.clone(), id.to_string()));
        }
        let body = mail.body_text(0).unwrap_or_default();
        let mut text = unquoted(&body);
        if text.is_empty() {
            text.clone_from(&subject);
        }
        let files: Vec<_> = mail
            .attachments()
            .filter(|part| {
                part.content_type()
                    .is_some_and(|t| t.ctype().eq_ignore_ascii_case("image"))
            })
            .map(|part| part.contents().to_vec())
            .collect();
        if text.is_empty() && files.is_empty() {
            return None;
        }
        Some(Incoming {
            platform: Platform::Email,
            chat: address.clone(),
            user: address.clone(),
            name: from.name().map_or(address, str::to_string),
            text,
            files,
        })
    }

    /// The mail that answers `to` with `text`, as it goes over SMTP after `DATA`.
    fn compose(&self, to: &str, text: &str) -> String {
        let last = self
            .threads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(to)
            .cloned();
        let subject = match &last {
            Some((subject, _)) if subject.to_lowercase().starts_with("re:") => subject.clone(),
            Some((subject, _)) if !subject.is_empty() => format!("Re: {subject}"),
            _ => "Patchwork".to_string(),
        };
        let mut mail = String::new();
        let _ = write!(
            mail,
            "From: <{}>\r\nTo: <{to}>\r\nSubject: {}\r\nDate: {}\r\nMessage-ID: <{}@patchwork>\r\n",
            self.address,
            header_text(&subject),
            chrono::Local::now().to_rfc2822(),
            uuid::Uuid::new_v4()
        );
        if let Some((_, id)) = &last {
            let id = id.replace(['\r', '\n', '<', '>'], "");
            let _ = write!(mail, "In-Reply-To: <{id}>\r\nReferences: <{id}>\r\n");
        }
        mail.push_str(
            "MIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\
             Content-Transfer-Encoding: base64\r\n\r\n",
        );
        let body = STANDARD.encode(text.replace("\r\n", "\n").replace('\n', "\r\n"));
        for line in body.as_bytes().chunks(BASE64_LINE) {
            mail.push_str(&String::from_utf8_lossy(line));
            mail.push_str("\r\n");
        }
        // A base64 body never starts a line with a dot, so it needs no dot-stuffing.
        mail.push_str(".\r\n");
        mail
    }
}

impl Connector for Email {
    fn listen(&self, to: &Listener<'_>) -> Result<(), Error> {
        let (host, port) = &self.imap;
        let stream = open("IMAP", host, *port, true, READ_WAIT)?;
        let mut imap = Imap {
            lines: Lines::new("The IMAP server", stream),
            tag: 0,
        };
        let greeting = imap.lines.line_within(ANSWER_WAIT)?;
        if !greeting.starts_with("* OK") {
            return Err(format!("The IMAP server refused the connection: {greeting}").into());
        }
        imap.command(&format!(
            "LOGIN {} {}",
            quote(&self.address),
            quote(&self.password)
        ))?;
        let (selected, _) = imap.command("SELECT INBOX")?;
        let mut next = {
            let mut kept = self.next.lock().unwrap_or_else(PoisonError::into_inner);
            *kept.get_or_insert_with(|| uid_next(&selected).unwrap_or(1))
        };
        (to.connected)(self.address.clone());
        while !to.stop.load(Ordering::Relaxed) {
            let (found, _) = imap.command(&format!("UID SEARCH UID {next}:*"))?;
            let mut uids: Vec<u32> = found
                .iter()
                .filter_map(|l| l.strip_prefix("* SEARCH"))
                .flat_map(str::split_whitespace)
                .filter_map(|n| n.parse().ok())
                .filter(|n| *n >= next)
                .collect();
            uids.sort_unstable();
            for uid in uids {
                let (_, bodies) = imap.command(&format!("UID FETCH {uid} BODY.PEEK[]"))?;
                if let Some(message) = bodies.first().and_then(|raw| self.incoming(raw)) {
                    (to.inbox)(message);
                }
                imap.command(&format!("UID STORE {uid} +FLAGS (\\Seen)"))?;
                next = uid + 1;
                *self.next.lock().unwrap_or_else(PoisonError::into_inner) = Some(next);
            }
            pause(POLL, to.stop);
        }
        let _ = imap.command("LOGOUT");
        Ok(())
    }

    fn send(&self, chat: &str, text: &str, _buttons: &[Button]) -> Result<Option<String>, Error> {
        if chat.contains(['\r', '\n', '<', '>', ' ']) || !chat.contains('@') {
            return Err(format!("{chat} is not an email address").into());
        }
        let (host, port) = &self.smtp;
        let starttls = STARTTLS_PORTS.contains(port);
        let stream = open("SMTP", host, *port, !starttls, READ_WAIT)?;
        let mut smtp = Lines::new("The SMTP server", stream);
        expect(&mut smtp, "220")?;
        smtp.send("EHLO patchwork")?;
        expect(&mut smtp, "250")?;
        if starttls {
            smtp.send("STARTTLS")?;
            expect(&mut smtp, "220")?;
            let stream = smtp.into_stream().start_tls("SMTP", host)?;
            smtp = Lines::new("The SMTP server", stream);
            smtp.send("EHLO patchwork")?;
            expect(&mut smtp, "250")?;
        }
        let login = STANDARD.encode(format!("\0{}\0{}", self.address, self.password));
        smtp.send(&format!("AUTH PLAIN {login}"))?;
        expect(&mut smtp, "235")?;
        smtp.send(&format!("MAIL FROM:<{}>", self.address))?;
        expect(&mut smtp, "250")?;
        smtp.send(&format!("RCPT TO:<{chat}>"))?;
        expect(&mut smtp, "25")?;
        smtp.send("DATA")?;
        expect(&mut smtp, "354")?;
        smtp.send_raw(self.compose(chat, text).as_bytes())?;
        expect(&mut smtp, "250")?;
        let _ = smtp.send("QUIT");
        Ok(None)
    }

    fn edit(&self, _chat: &str, _id: &str, _text: &str) -> Result<(), Error> {
        Err("Mail cannot be edited".into())
    }

    fn notes(&self) -> bool {
        false
    }
}

/// An IMAP session, numbering its commands.
struct Imap {
    lines: Lines,
    tag: u32,
}

impl Imap {
    /// Runs `command` and returns the lines the server answered with and the bodies sent in them.
    fn command(&mut self, command: &str) -> Result<(Vec<String>, Vec<Vec<u8>>), Error> {
        self.tag += 1;
        let tag = format!("p{} ", self.tag);
        self.lines.send(&format!("{tag}{command}"))?;
        let mut lines = Vec::new();
        let mut bodies = Vec::new();
        loop {
            let line = self.lines.line_within(ANSWER_WAIT)?;
            if let Some(answer) = line.strip_prefix(&tag) {
                if answer.starts_with("OK") {
                    return Ok((lines, bodies));
                }
                // The command's first word only, so a password never reaches the message.
                let name = command.split(' ').next().unwrap_or_default();
                return Err(format!("The IMAP server refused {name}: {answer}").into());
            }
            if let Some(size) = literal(&line) {
                bodies.push(self.lines.bytes_within(size, ANSWER_WAIT)?);
            }
            lines.push(line);
        }
    }
}

/// The size of the body that follows `line`, when it ends with `{size}`.
fn literal(line: &str) -> Option<usize> {
    line.strip_suffix('}')?.rsplit_once('{')?.1.parse().ok()
}

/// The `UIDNEXT` a `SELECT` answered with.
fn uid_next(lines: &[String]) -> Option<u32> {
    lines.iter().find_map(|l| {
        let rest = l.split_once("[UIDNEXT ")?.1;
        rest.split(']').next()?.trim().parse().ok()
    })
}

/// `text` as an IMAP quoted string.
fn quote(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Reads an SMTP answer and checks that its code starts with `code`.
fn expect(smtp: &mut Lines, code: &str) -> Result<(), Error> {
    loop {
        let line = smtp.line_within(ANSWER_WAIT)?;
        // `250-` lines go on; the last line has a space after its code.
        if line.as_bytes().get(3) == Some(&b'-') {
            continue;
        }
        if line.starts_with(code) {
            return Ok(());
        }
        return Err(format!("The SMTP server refused the mail: {line}").into());
    }
}

/// Whether `mail` was sent by a program, such as an auto-reply or a mailing list.
fn automatic(mail: &Message<'_>) -> bool {
    let header = |name: &str| {
        mail.header(name)
            .and_then(|h| h.as_text())
            .map(str::to_lowercase)
    };
    header("Auto-Submitted").is_some_and(|v| v.trim() != "no")
        || header("Precedence").is_some_and(|v| ["bulk", "list", "junk"].contains(&v.trim()))
        || mail.header("List-Id").is_some()
}

/// Whether the receiving server found that `mail` really comes from its `From` domain.
fn proven(mail: &Message<'_>) -> bool {
    // The top Authentication-Results header is the one the receiving server added.
    mail.header("Authentication-Results")
        .and_then(|h| h.as_text())
        .is_some_and(|v| v.to_lowercase().contains("dmarc=pass"))
}

/// `body` without the quoted mail a reply carries below it.
fn unquoted(body: &str) -> String {
    let mut kept = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        let quote_starts = trimmed.starts_with('>')
            || (trimmed.starts_with("On ") && trimmed.ends_with("wrote:"))
            || trimmed.starts_with("-----Original Message-----");
        if quote_starts {
            break;
        }
        kept.push(line);
    }
    kept.join("\n").trim().to_string()
}

/// `text` as a header value: one line, encoded when it is not plain ASCII.
fn header_text(text: &str) -> String {
    let text = text.replace(['\r', '\n'], " ");
    if text.is_ascii() {
        text
    } else {
        format!("=?UTF-8?B?{}?=", STANDARD.encode(text))
    }
}
