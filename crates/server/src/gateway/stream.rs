//! Line-based connections over TCP, with or without TLS, that IRC and email talk over.

// Rust guideline compliant 2026-10-09

use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};

use super::net::failed;
use crate::Error;

/// How long connecting and the TLS handshake may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// The longest line a server may send, in bytes.
const MAX_LINE: usize = 1024 * 1024;

/// A TCP connection, encrypted or not.
pub(crate) enum Stream {
    Plain(TcpStream),
    Tls(Box<StreamOwned<ClientConnection, TcpStream>>),
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(s) => s.read(buf),
            Self::Tls(s) => s.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(s) => s.write(buf),
            Self::Tls(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Plain(s) => s.flush(),
            Self::Tls(s) => s.flush(),
        }
    }
}

/// `address` split into its host and port, with `port` when it names none.
pub(crate) fn host_port(address: &str, port: u16) -> Result<(String, u16), Error> {
    let address = address.trim();
    // A bare IPv6 address has colons of its own, so only a bracketed one names a port.
    let split = match address.strip_prefix('[') {
        Some(rest) => rest
            .split_once(']')
            .map(|(host, tail)| (host, tail.strip_prefix(':'))),
        None if address.matches(':').count() == 1 => address
            .split_once(':')
            .map(|(host, number)| (host, Some(number))),
        None => None,
    };
    match split {
        Some((host, Some(number))) => {
            let number = number
                .parse()
                .map_err(|_bad| format!("{address} has no valid port"))?;
            Ok((host.to_string(), number))
        }
        Some((host, None)) => Ok((host.to_string(), port)),
        None => Ok((address.to_string(), port)),
    }
}

/// Connects to `host` on `port`, over TLS when `tls` is set, with reads that give up after
/// `wait`.
///
/// # Errors
///
/// Returns an error when the server cannot be reached or the TLS handshake fails.
pub(crate) fn open(
    what: &str,
    host: &str,
    port: u16,
    tls: bool,
    wait: Duration,
) -> Result<Stream, Error> {
    let socket = std::net::ToSocketAddrs::to_socket_addrs(&(host, port))
        .map_err(|e| failed(what, e, ""))?
        .find_map(|address| TcpStream::connect_timeout(&address, CONNECT_TIMEOUT).ok())
        .ok_or_else(|| format!("{what}: could not connect to {host}:{port}"))?;
    let stream = Stream::Plain(socket);
    let stream = if tls {
        stream.start_tls(what, host)?
    } else {
        stream
    };
    stream.set_wait(what, wait)?;
    Ok(stream)
}

impl Stream {
    /// The same connection, encrypted with TLS for `host`.
    ///
    /// # Errors
    ///
    /// Returns an error when the TLS handshake fails.
    pub(crate) fn start_tls(self, what: &str, host: &str) -> Result<Self, Error> {
        let Self::Plain(mut socket) = self else {
            return Ok(self);
        };
        let roots = RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let config =
            ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .map_err(|e| failed(what, e, ""))?
                .with_root_certificates(roots)
                .with_no_client_auth();
        let name = ServerName::try_from(host.to_string()).map_err(|e| failed(what, e, ""))?;
        let mut connection =
            ClientConnection::new(Arc::new(config), name).map_err(|e| failed(what, e, ""))?;
        socket
            .set_read_timeout(Some(CONNECT_TIMEOUT))
            .map_err(|e| failed(what, e, ""))?;
        while connection.is_handshaking() {
            connection
                .complete_io(&mut socket)
                .map_err(|e| failed(what, e, ""))?;
        }
        Ok(Self::Tls(Box::new(StreamOwned::new(connection, socket))))
    }

    fn set_wait(&self, what: &str, wait: Duration) -> Result<(), Error> {
        let socket = match self {
            Self::Plain(s) => s,
            Self::Tls(s) => s.get_ref(),
        };
        socket
            .set_read_timeout(Some(wait))
            .map_err(|e| failed(what, e, ""))
    }
}

/// A connection read a line at a time.
pub(crate) struct Lines {
    what: &'static str,
    stream: Stream,
    buf: Vec<u8>,
}

impl Lines {
    pub(crate) fn new(what: &'static str, stream: Stream) -> Self {
        Self {
            what,
            stream,
            buf: Vec::new(),
        }
    }

    /// The connection, for a protocol that switches to TLS part way.
    pub(crate) fn into_stream(self) -> Stream {
        self.stream
    }

    /// Reads once more from the connection; false when nothing arrived in time.
    fn fill(&mut self) -> Result<bool, Error> {
        let mut chunk = [0_u8; 8192];
        match self.stream.read(&mut chunk) {
            Ok(0) => Err(format!("{} closed the connection", self.what).into()),
            Ok(n) => {
                self.buf.extend_from_slice(&chunk[..n]);
                Ok(true)
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => Ok(false),
            Err(e) => Err(failed(self.what, e, "")),
        }
    }

    /// The next line without its line break, or none when no whole line arrived in time.
    ///
    /// # Errors
    ///
    /// Returns an error when the connection closes or fails.
    pub(crate) fn line(&mut self) -> Result<Option<String>, Error> {
        loop {
            if let Some(at) = self.buf.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = self.buf.drain(..=at).collect();
                let line = String::from_utf8_lossy(&line);
                return Ok(Some(line.trim_end_matches(['\r', '\n']).to_string()));
            }
            if self.buf.len() > MAX_LINE {
                return Err(format!("{} sent a line that is too long", self.what).into());
            }
            if !self.fill()? {
                return Ok(None);
            }
        }
    }

    /// The next line, waiting up to `wait` for it.
    ///
    /// # Errors
    ///
    /// Returns an error when the connection closes or fails, or no line arrives in time.
    pub(crate) fn line_within(&mut self, wait: Duration) -> Result<String, Error> {
        let until = Instant::now() + wait;
        loop {
            if let Some(line) = self.line()? {
                return Ok(line);
            }
            if Instant::now() >= until {
                return Err(format!("{} stopped answering", self.what).into());
            }
        }
    }

    /// The next `count` bytes, waiting up to `wait` for them.
    ///
    /// # Errors
    ///
    /// Returns an error when the connection closes or fails, or the bytes do not arrive in time.
    pub(crate) fn bytes_within(&mut self, count: usize, wait: Duration) -> Result<Vec<u8>, Error> {
        let until = Instant::now() + wait;
        while self.buf.len() < count {
            if !self.fill()? && Instant::now() >= until {
                return Err(format!("{} stopped answering", self.what).into());
            }
        }
        Ok(self.buf.drain(..count).collect())
    }

    /// Sends `line` with a line break after it.
    ///
    /// # Errors
    ///
    /// Returns an error when the connection fails.
    pub(crate) fn send(&mut self, line: &str) -> Result<(), Error> {
        self.send_raw(format!("{line}\r\n").as_bytes())
    }

    /// Sends `bytes` as they are.
    ///
    /// # Errors
    ///
    /// Returns an error when the connection fails.
    pub(crate) fn send_raw(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.stream
            .write_all(bytes)
            .and_then(|()| self.stream.flush())
            .map_err(|e| failed(self.what, e, ""))
    }
}
