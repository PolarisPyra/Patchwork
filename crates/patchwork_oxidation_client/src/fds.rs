//! Unix `SCM_RIGHTS` file descriptor handover shared by the Oxidation host and client SDK.
//!
//! When handoff is enabled on Unix, the host creates a `SOCK_SEQPACKET` Unix socket pair and
//! passes one end to the child process via `OXIDATION_FDS`. Before exiting on `Restart`, the
//! child sends live descriptors (such as pseudo-terminal masters) across the socket; the host
//! holds them open across the relaunch and forwards them to the next instance.

// Rust guideline compliant 2026-10-09

/// Descriptors handed over by a running application and held across a restart.
#[derive(Debug, Default)]
pub struct HeldFds {
    #[cfg(unix)]
    fds: Vec<std::os::fd::OwnedFd>,
}

impl HeldFds {
    /// Returns the number of held descriptors.
    #[must_use]
    pub fn len(&self) -> usize {
        #[cfg(unix)]
        {
            self.fds.len()
        }
        #[cfg(not(unix))]
        {
            0
        }
    }

    /// Returns whether no descriptors are held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(unix)]
impl HeldFds {
    /// Wraps a vector of owned file descriptors.
    #[must_use]
    pub fn from_fds(fds: Vec<std::os::fd::OwnedFd>) -> Self {
        Self { fds }
    }

    /// Consumes the container and returns the owned descriptors.
    #[must_use]
    pub fn into_fds(self) -> Vec<std::os::fd::OwnedFd> {
        self.fds
    }

    /// Borrows the held descriptors.
    #[must_use]
    pub fn as_slice(&self) -> &[std::os::fd::OwnedFd] {
        &self.fds
    }
}

#[cfg(unix)]
pub use unix::{FdChannel, pair, receive_fds, receive_fds_nowait, send_fds};

#[cfg(unix)]
mod unix {
    use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
    use std::process::Command;
    use std::time::Duration;

    use crate::protocol::FDS_VAR;

    use super::HeldFds;

    /// Maximum descriptors carried per `SCM_RIGHTS` message (the kernel limit is 253).
    const PER_MESSAGE: usize = 200;

    /// Host-side Unix socket channel used to pass descriptors to and from one child process.
    #[derive(Debug)]
    pub struct FdChannel {
        own: OwnedFd,
    }

    impl FdChannel {
        /// Wraps an already-open Unix socket descriptor.
        #[must_use]
        pub fn from_owned(own: OwnedFd) -> Self {
            Self { own }
        }

        /// Sends `held` to the child process.
        ///
        /// # Errors
        ///
        /// Returns an I/O error if `sendmsg` fails.
        pub fn give(&self, held: &HeldFds) -> std::io::Result<()> {
            send_fds(self.own.as_fd(), &held.fds)
        }

        /// Drains all descriptors currently queued from the child process without blocking.
        ///
        /// # Errors
        ///
        /// Returns an I/O error if `recvmsg` fails.
        pub fn take(&self) -> std::io::Result<HeldFds> {
            let mut fds = Vec::new();
            while let Some(more) = receive_fds_nowait(self.own.as_fd())? {
                fds.extend(more);
            }
            Ok(HeldFds { fds })
        }
    }

    /// Creates a `SOCK_SEQPACKET` Unix socket pair and configures `command` to inherit the
    /// child's end with its numeric descriptor stored in `OXIDATION_FDS`.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if `socketpair` fails.
    pub fn pair(command: &mut Command) -> std::io::Result<(FdChannel, OwnedFd)> {
        let mut ends = [RawFd::default(); 2];
        // SAFETY: `ends` has room for the two descriptors `socketpair` writes.
        let made = unsafe {
            libc::socketpair(
                libc::AF_UNIX,
                libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
                0,
                ends.as_mut_ptr(),
            )
        };
        if made != 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: `socketpair` just opened both descriptors and nothing else owns them.
        let (own, theirs) =
            unsafe { (OwnedFd::from_raw_fd(ends[0]), OwnedFd::from_raw_fd(ends[1])) };
        let raw = theirs.as_raw_fd();
        command.env(FDS_VAR, raw.to_string());
        // SAFETY: the hook calls only `fcntl` (async-signal-safe) on a descriptor open in the child.
        unsafe {
            std::os::unix::process::CommandExt::pre_exec(command, move || {
                let flags = libc::fcntl(raw, libc::F_GETFD);
                if flags < 0 || libc::fcntl(raw, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        };
        Ok((FdChannel { own }, theirs))
    }

    /// Sends `fds` over Unix socket `socket`, chunked to stay within kernel `SCM_RIGHTS` limits.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if `sendmsg` fails.
    #[cfg_attr(
        all(target_os = "linux", target_env = "gnu"),
        expect(
            trivial_numeric_casts,
            reason = "cmsg length fields are narrower types on other platforms"
        )
    )]
    pub fn send_fds(socket: BorrowedFd<'_>, fds: &[OwnedFd]) -> std::io::Result<()> {
        for chunk in fds.chunks(PER_MESSAGE) {
            let raw: Vec<RawFd> = chunk.iter().map(AsRawFd::as_raw_fd).collect();
            let mut control = control_buffer(raw.len());
            let mut byte = [0u8; 1];
            let mut iov = libc::iovec {
                iov_base: byte.as_mut_ptr().cast(),
                iov_len: byte.len(),
            };
            // SAFETY: `msghdr` is plain data; all required fields are initialized below.
            let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
            message.msg_iov = &raw mut iov;
            message.msg_iovlen = 1;
            message.msg_control = control.as_mut_ptr().cast();
            message.msg_controllen = std::mem::size_of_val(control.as_slice()) as _;
            // SAFETY: `control` is aligned for `cmsghdr` and sized for `raw.len()` descriptors.
            unsafe {
                let header = libc::CMSG_FIRSTHDR(&raw const message);
                (*header).cmsg_level = libc::SOL_SOCKET;
                (*header).cmsg_type = libc::SCM_RIGHTS;
                (*header).cmsg_len = libc::CMSG_LEN(fd_bytes(raw.len())) as _;
                std::ptr::copy_nonoverlapping(
                    raw.as_ptr(),
                    libc::CMSG_DATA(header).cast::<RawFd>(),
                    raw.len(),
                );
            };
            // SAFETY: `message` references live stack/heap buffers for the duration of the call.
            if unsafe { libc::sendmsg(socket.as_raw_fd(), &raw const message, libc::MSG_NOSIGNAL) }
                < 0
            {
                return Err(std::io::Error::last_os_error());
            }
        }
        Ok(())
    }

    /// Receives the next descriptor message from `socket`, waiting up to `within`.
    ///
    /// Returns `Ok(None)` on timeout or peer close.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if `poll` or `recvmsg` fails.
    pub fn receive_fds(
        socket: BorrowedFd<'_>,
        within: Duration,
    ) -> std::io::Result<Option<Vec<OwnedFd>>> {
        let mut poll = libc::pollfd {
            fd: socket.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let millis = libc::c_int::try_from(within.as_millis()).unwrap_or(libc::c_int::MAX);
        // SAFETY: `poll` points to one valid `pollfd`.
        match unsafe { libc::poll(&raw mut poll, 1, millis) } {
            0 => return Ok(None),
            n if n < 0 => return Err(std::io::Error::last_os_error()),
            _ => {}
        }
        receive_fds_nowait(socket)
    }

    /// Receives the next descriptor message from `socket` without blocking.
    ///
    /// Returns `Ok(None)` when no message is waiting or the peer has closed.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if `recvmsg` fails for any reason other than `WouldBlock`.
    #[cfg_attr(
        all(target_os = "linux", target_env = "gnu"),
        expect(
            trivial_numeric_casts,
            reason = "cmsg length fields are narrower types on other platforms"
        )
    )]
    pub fn receive_fds_nowait(socket: BorrowedFd<'_>) -> std::io::Result<Option<Vec<OwnedFd>>> {
        let mut control = control_buffer(PER_MESSAGE);
        let mut byte = [0u8; 1];
        let mut iov = libc::iovec {
            iov_base: byte.as_mut_ptr().cast(),
            iov_len: byte.len(),
        };
        // SAFETY: `msghdr` is plain data; all required fields are initialized below.
        let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
        message.msg_iov = &raw mut iov;
        message.msg_iovlen = 1;
        message.msg_control = control.as_mut_ptr().cast();
        message.msg_controllen = std::mem::size_of_val(control.as_slice()) as _;
        // SAFETY: `message` references live stack/heap buffers for the duration of the call.
        let read = unsafe {
            libc::recvmsg(
                socket.as_raw_fd(),
                &raw mut message,
                libc::MSG_CMSG_CLOEXEC | libc::MSG_DONTWAIT,
            )
        };
        if read < 0 {
            let e = std::io::Error::last_os_error();
            return match e.kind() {
                std::io::ErrorKind::WouldBlock => Ok(None),
                _ => Err(e),
            };
        }
        if read == 0 {
            return Ok(None);
        }
        let mut fds = Vec::new();
        // SAFETY: the kernel populated `message`'s control headers; each received `SCM_RIGHTS`
        // descriptor is newly opened in this process with sole ownership.
        unsafe {
            let mut header = libc::CMSG_FIRSTHDR(&raw const message);
            while !header.is_null() {
                if (*header).cmsg_level == libc::SOL_SOCKET
                    && (*header).cmsg_type == libc::SCM_RIGHTS
                {
                    let data = libc::CMSG_DATA(header).cast::<RawFd>();
                    let len = (*header).cmsg_len as usize - libc::CMSG_LEN(0) as usize;
                    for i in 0..len / std::mem::size_of::<RawFd>() {
                        fds.push(OwnedFd::from_raw_fd(data.add(i).read_unaligned()));
                    }
                }
                header = libc::CMSG_NXTHDR(&raw const message, header);
            }
        }
        Ok(Some(fds))
    }

    fn fd_bytes(count: usize) -> u32 {
        u32::try_from(count * std::mem::size_of::<RawFd>()).unwrap_or(u32::MAX)
    }

    fn control_buffer(count: usize) -> Vec<u64> {
        // SAFETY: `CMSG_SPACE` is a pure size calculation.
        let space = unsafe { libc::CMSG_SPACE(fd_bytes(count)) } as usize;
        vec![0u64; space.div_ceil(std::mem::size_of::<u64>())]
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::{Read, Write};

        #[test]
        fn passes_descriptors_through_channel() {
            let mut command = Command::new("true");
            let (ours, theirs) = pair(&mut command).expect("a socket pair");
            let app = FdChannel::from_owned(theirs);
            let (mut reader, writer) = std::io::pipe().expect("a pipe");
            let held = HeldFds::from_fds(vec![OwnedFd::from(writer)]);
            app.give(&held).expect("sent");
            // Only the copy sent across the socket may keep the write end open.
            drop(held);
            let taken = ours.take().expect("taken");
            assert_eq!(taken.len(), 1);
            let mut writer =
                std::fs::File::from(taken.into_fds().into_iter().next().expect("one fd"));
            writer.write_all(b"held").expect("write");
            drop(writer);
            let mut text = String::new();
            reader.read_to_string(&mut text).expect("read");
            assert_eq!(text, "held");
        }

        #[test]
        fn receive_fds_respects_timeout_and_reads_sent_fds() {
            let mut command = Command::new("true");
            let (ours, theirs) = pair(&mut command).expect("a socket pair");
            assert!(
                receive_fds(theirs.as_fd(), Duration::from_millis(5))
                    .expect("poll")
                    .is_none()
            );
            let (mut reader, writer) = std::io::pipe().expect("a pipe");
            send_fds(ours.own.as_fd(), &[OwnedFd::from(writer)]).expect("send");
            let received = receive_fds(theirs.as_fd(), Duration::from_secs(1))
                .expect("recv")
                .expect("fds");
            assert_eq!(received.len(), 1);
            let mut file = std::fs::File::from(received.into_iter().next().expect("fd"));
            file.write_all(b"ok").expect("write");
            drop(file);
            let mut out = String::new();
            reader.read_to_string(&mut out).expect("read");
            assert_eq!(out, "ok");
        }
    }
}
