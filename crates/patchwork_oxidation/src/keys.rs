//! Single keys typed in the terminal: `r` rebuilds, `c` clears and `q` quits.
//!
//! The terminal is put in a mode where each key arrives as it is typed and isn't echoed, and is
//! put back as it was when the returned guard is dropped. Ctrl-C still works as a signal.

// Rust guideline compliant 2026-10-09

use std::io::{IsTerminal, Read};
use std::sync::mpsc::Sender;

use crate::Event;

/// A key Oxidation acts on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// Rebuild and restart now, even with nothing saved.
    Rebuild,
    /// Clear the terminal.
    Clear,
    /// Stop the app and quit.
    Quit,
}

impl Key {
    fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            b'r' | b'R' => Some(Self::Rebuild),
            b'c' | b'C' => Some(Self::Clear),
            b'q' | b'Q' => Some(Self::Quit),
            _ => None,
        }
    }
}

/// Puts the terminal back as it was when dropped.
#[derive(Debug)]
pub struct Keys {
    #[cfg(unix)]
    before: Option<libc::termios>,
}

impl Drop for Keys {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(before) = self.before {
            // SAFETY: `before` is the state `tcgetattr` filled in for this same descriptor.
            unsafe {
                libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw const before);
            }
        }
    }
}

impl Keys {
    /// Whether keys are being read.
    #[must_use]
    pub fn active(&self) -> bool {
        #[cfg(unix)]
        return self.before.is_some();
        #[cfg(not(unix))]
        false
    }
}

/// Reads keys from standard input and sends them, when it is a terminal.
#[must_use]
pub fn listen(send: Sender<Event>) -> Keys {
    if !std::io::stdin().is_terminal() {
        return Keys {
            #[cfg(unix)]
            before: None,
        };
    }
    let keys = Keys {
        #[cfg(unix)]
        before: unbuffer(),
    };
    std::thread::spawn(move || {
        let mut byte = [0_u8; 1];
        let mut stdin = std::io::stdin().lock();
        while stdin.read_exact(&mut byte).is_ok() {
            if let Some(key) = Key::from_byte(byte[0])
                && send.send(Event::Key(key)).is_err()
            {
                return;
            }
        }
    });
    keys
}

/// Turns off line buffering and echo on standard input, returning the state to put back.
#[cfg(unix)]
fn unbuffer() -> Option<libc::termios> {
    // SAFETY: `termios` is plain data; `tcgetattr` fills it in before it is read.
    let mut state: libc::termios = unsafe { std::mem::zeroed() };
    // SAFETY: the descriptor is standard input and `state` is a valid, writable `termios`.
    if unsafe { libc::tcgetattr(libc::STDIN_FILENO, &raw mut state) } != 0 {
        return None;
    }
    let before = state;
    state.c_lflag &= !(libc::ICANON | libc::ECHO);
    state.c_cc[libc::VMIN] = 1;
    state.c_cc[libc::VTIME] = 0;
    // SAFETY: as above, with `state` read from this descriptor and changed only in its flags.
    if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw const state) } != 0 {
        return None;
    }
    Some(before)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_three_keys_in_either_case() {
        assert_eq!(Key::from_byte(b'r'), Some(Key::Rebuild));
        assert_eq!(Key::from_byte(b'C'), Some(Key::Clear));
        assert_eq!(Key::from_byte(b'q'), Some(Key::Quit));
        assert_eq!(Key::from_byte(b'x'), None);
    }
}
