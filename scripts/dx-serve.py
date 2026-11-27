#!/usr/bin/env python3
"""Runs `dx serve` in a pseudo-terminal with function keys for its shortcuts.

Usage (scripts/cargo-runner.sh runs it for `cargo run desktop`):
    uv run --no-project python3 scripts/dx-serve.py [DX ARGS...]

dx reads single letters from its terminal. This script sits between the terminal and dx:

- Each save is patched into the open app as it lands. F5 applies changes by hand: it has dx
  patch every Rust file saved since the last F5, in case a save was missed.
- F7 pauses and resumes hot reload (dx's `p`). While it is paused dx ignores saves; F5 patches
  them into the open app and stays paused, and resuming patches them too.
- Shift+F5 rebuilds and restarts the app (dx's `r`), F8 toggles verbose logging (`v`) and F1
  opens dx's other commands (`/`). Every other key, ctrl+c included, goes to dx as typed.
- dx's banner shows the function keys in place of the letters.
- The app drives the same shortcuts over a Unix socket whose path is in `PATCHWORK_DX_CONTROL`.
  It sends `apply`, `restart` or `toggle-paused`, one per line, and gets `paused` or `watching`
  back on connecting and whenever that changes.

dx patches only a file it sees change, so F5 rewrites each Rust file saved since the last F5
with the bytes it already holds; dx skips the ones it has already patched. While paused it
first resumes dx and waits for dx to say so, and pauses dx again once dx has taken them.
"""

from __future__ import annotations

import errno
import fcntl
import os
import pty
import select
import signal
import socket
import sys
import termios
import time
import tty

# Shift+F5, F7, F8 and F1 as terminals send them, and the dx letter each one stands for.
KEYS = {
    b"\x1b[15;2~": b"r",
    b"\x1bOP": b"/",
    b"\x1b[11~": b"/",
    b"\x1b[18~": b"p",
    b"\x1b[19~": b"v",
}
# F5, which this script handles itself.
APPLY = b"\x1b[15~"
SEQUENCES = sorted([*KEYS, APPLY], key=len, reverse=True)

# dx prints each shortcut as "`key`", in yellow, then what it does.
BANNER = {
    b"`r`\x1b[39;49m to rebuild the app": b"`shift+F5`\x1b[39;49m to rebuild and restart the app",
    b"`p`\x1b[39;49m to toggle automatic rebuilds": b"`F7`\x1b[39;49m to pause hot reload and "
    b"\x1b[33;49m`F5`\x1b[39;49m to apply changes",
    b"`v`\x1b[39;49m to toggle": b"`F8`\x1b[39;49m to toggle",
    b"`/`\x1b[39;49m for more": b"`F1`\x1b[39;49m for more",
}

COMMANDS = {b"apply", b"restart", b"toggle-paused"}

# What dx logs when it resumes or pauses.
TOGGLED = b"Automatic rebuilds are currently"
# How long to wait for dx to say it resumed before rewriting the files anyway.
RESUME_WAIT = 1.0
# How long dx gets to take the rewritten files before it is paused again.
SETTLE = 1.0
# Folders that hold nothing the app builds from.
SKIP = {"target", "node_modules"}


def held_back(data: bytes, patterns: list[bytes]) -> int:
    """How many bytes at the end of `data` could start one of `patterns`."""
    longest = 0
    for pattern in patterns:
        for size in range(min(len(pattern) - 1, len(data)), longest, -1):
            if data.endswith(pattern[:size]):
                longest = size
                break
    return longest


def window_size(fd: int) -> bytes | None:
    try:
        return fcntl.ioctl(fd, termios.TIOCGWINSZ, b"\0" * 8)
    except OSError:
        return None


def write_all(fd: int, data: bytes) -> None:
    while data:
        data = data[os.write(fd, data) :]


def saved_since(root: str, since: float) -> list[str]:
    """The Rust files under `root` changed after `since`."""
    found = []
    for folder, dirs, files in os.walk(root):
        dirs[:] = [d for d in dirs if d not in SKIP and not d.startswith(".")]
        for name in files:
            if not name.endswith(".rs"):
                continue
            path = os.path.join(folder, name)
            try:
                if os.stat(path).st_mtime > since:
                    found.append(path)
            except OSError:
                pass
    return found


def rewrite(path: str) -> None:
    """Writes `path` over with its own bytes, which the watcher sees as a save."""
    try:
        with open(path, "r+b") as f:
            data = f.read()
            f.seek(0)
            f.write(data)
    except OSError:
        pass


class Serve:
    def __init__(self, master: int, path: str, root: str) -> None:
        self.master = master
        self.root = root
        # Whether hot reload is paused, as the user sees it.
        self.paused = False
        # Since when F5 looks for saves: the start, or the last time it applied them.
        self.since = time.time()
        # Files to rewrite once dx says it resumed, and when to rewrite them anyway.
        self.to_rewrite: list[str] = []
        self.rewrite_at: float | None = None
        # Whether dx is to be paused again once it has taken the rewritten files.
        self.repause = False
        # While dx takes rewritten files during a pause, when to pause it again.
        self.repause_at: float | None = None
        # The end of dx's output, to find `TOGGLED` split across reads.
        self.tail = b""
        self.input = b""
        self.output = b""
        self.clients: dict[int, tuple[socket.socket, bytes]] = {}
        self.server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.server.bind(path)
        self.server.listen()
        self.server.setblocking(False)

    def dx_watching(self) -> bool:
        """Whether dx is resumed in the middle of a pause, for an apply."""
        return self.rewrite_at is not None or self.repause_at is not None

    def replay(self, repause: bool) -> bool:
        """Resumes dx and has it see the saves held by the pause. Returns whether there were any."""
        files = saved_since(self.root, self.since)
        if not files:
            return False
        self.repause = repause
        if self.rewrite_at is not None:
            self.to_rewrite += files
        elif self.repause_at is not None:
            self.to_rewrite = files
            self.rewrite_all()
        else:
            write_all(self.master, b"p")
            self.to_rewrite = files
            self.rewrite_at = time.monotonic() + RESUME_WAIT
        return True

    def rewrite_all(self) -> None:
        """Rewrites the held files now that dx watches, and has it paused after if asked."""
        for path in self.to_rewrite:
            rewrite(path)
        self.to_rewrite = []
        self.rewrite_at = None
        self.since = time.time()
        self.repause_at = time.monotonic() + SETTLE if self.repause else None

    def apply(self) -> None:
        if self.paused:
            self.replay(repause=True)
        elif not self.dx_watching():
            self.to_rewrite = saved_since(self.root, self.since)
            self.repause = False
            self.rewrite_all()

    def toggle(self) -> None:
        if self.paused:
            if self.dx_watching():
                # dx is already resumed for an apply; leave it so.
                self.repause = False
                self.repause_at = None
            elif not self.replay(repause=False):
                write_all(self.master, b"p")
        else:
            write_all(self.master, b"p")
        self.paused = not self.paused
        self.tell_all()

    def restart(self) -> None:
        write_all(self.master, b"r")
        # The rebuild takes every save, so none is left to apply.
        self.since = time.time()

    def tick(self) -> float | None:
        """Runs the apply's timed steps that are due; returns how long until the next one."""
        now = time.monotonic()
        if self.rewrite_at is not None:
            if now < self.rewrite_at:
                return self.rewrite_at - now
            self.rewrite_all()
        if self.repause_at is not None:
            if now < self.repause_at:
                return self.repause_at - now
            write_all(self.master, b"p")
            self.repause_at = None
        return None

    def keys(self, data: bytes) -> None:
        """Passes keys to dx, handling the ones this script owns along the way."""
        plain = b""
        i = 0
        while i < len(data):
            sequence = next((s for s in SEQUENCES if data.startswith(s, i)), None)
            key = KEYS.get(sequence, sequence) if sequence else data[i : i + 1]
            i += len(sequence) if sequence else 1
            if key in (APPLY, b"p", b"r"):
                write_all(self.master, plain)
                plain = b""
                {APPLY: self.apply, b"p": self.toggle, b"r": self.restart}[key]()
            else:
                plain += key
        write_all(self.master, plain)

    def state(self) -> bytes:
        return b"paused\n" if self.paused else b"watching\n"

    def tell_all(self) -> None:
        for fd in list(self.clients):
            try:
                self.clients[fd][0].sendall(self.state())
            except OSError:
                self.drop(fd)

    def drop(self, fd: int) -> None:
        self.clients.pop(fd)[0].close()

    def accept(self) -> None:
        conn, _ = self.server.accept()
        try:
            conn.sendall(self.state())
        except OSError:
            conn.close()
            return
        self.clients[conn.fileno()] = (conn, b"")

    def from_client(self, fd: int) -> None:
        conn, pending = self.clients[fd]
        try:
            data = conn.recv(4096)
        except OSError:
            data = b""
        if not data:
            self.drop(fd)
            return
        *lines, pending = (pending + data).split(b"\n")
        self.clients[fd] = (conn, pending)
        for line in lines:
            command = line.strip()
            if command in COMMANDS:
                {b"apply": self.apply, b"restart": self.restart, b"toggle-paused": self.toggle}[
                    command
                ]()

    def from_terminal(self, data: bytes) -> None:
        data = self.input + data
        keep = held_back(data, SEQUENCES)
        self.input = data[len(data) - keep :]
        self.keys(data[: len(data) - keep])

    def flush_input(self) -> None:
        """Sends a lone escape, or the start of a sequence that never finished, as it is."""
        if self.input:
            data, self.input = self.input, b""
            self.keys(data)

    def from_dx(self, data: bytes) -> bytes:
        """dx's output with the banner's letters changed to function keys."""
        if self.rewrite_at is not None and TOGGLED in self.tail + data:
            self.rewrite_all()
        self.tail = (self.tail + data)[-len(TOGGLED) :]
        data = self.output + data
        for letter, key in BANNER.items():
            data = data.replace(letter, key)
        keep = held_back(data, list(BANNER))
        self.output = data[len(data) - keep :]
        return data[: len(data) - keep]

    def flush_output(self) -> bytes:
        data, self.output = self.output, b""
        return data


def main() -> int:
    stdin, stdout = sys.stdin.fileno(), sys.stdout.fileno()
    interactive = os.isatty(stdin)
    path = os.path.join(
        os.environ.get("XDG_RUNTIME_DIR") or "/tmp", f"patchwork-dx-{os.getpid()}.sock"
    )
    size = window_size(stdin) if interactive else None

    pid, master = pty.fork()
    if pid == 0:
        os.environ["PATCHWORK_DX_CONTROL"] = path
        os.execvp("dx", ["dx", "serve", *sys.argv[1:]])

    if size:
        fcntl.ioctl(master, termios.TIOCSWINSZ, size)

    def resize(*_: object) -> None:
        if size := window_size(stdin):
            fcntl.ioctl(master, termios.TIOCSWINSZ, size)
            os.kill(pid, signal.SIGWINCH)

    def leave(*_: object) -> None:
        raise SystemExit(1)

    signal.signal(signal.SIGWINCH, resize)
    # Without a terminal in raw mode, ctrl+c arrives as a signal; dx exits on its own key.
    signal.signal(signal.SIGINT, lambda *_: write_all(master, b"\x03"))
    signal.signal(signal.SIGTERM, leave)
    signal.signal(signal.SIGHUP, leave)
    saved = termios.tcgetattr(stdin) if interactive else None
    if interactive:
        tty.setraw(stdin)
    serve = Serve(master, path, os.getcwd())
    try:
        while True:
            fds = [master, serve.server, *serve.clients]
            if interactive:
                fds.append(stdin)
            timeouts = [t for t in (serve.tick(),) if t is not None]
            if serve.input or serve.output:
                timeouts.append(0.05)
            try:
                ready, _, _ = select.select(fds, [], [], min(timeouts, default=None))
            except InterruptedError:
                continue
            if not ready:
                serve.flush_input()
                write_all(stdout, serve.flush_output())
                continue
            if master in ready:
                try:
                    data = os.read(master, 65536)
                except OSError as e:
                    if e.errno != errno.EIO:
                        raise
                    data = b""
                if not data:
                    write_all(stdout, serve.flush_output())
                    break
                write_all(stdout, serve.from_dx(data))
            if stdin in ready:
                serve.from_terminal(os.read(stdin, 1024))
            if serve.server in ready:
                serve.accept()
            for fd in ready:
                if fd in serve.clients:
                    serve.from_client(fd)
    finally:
        if saved:
            termios.tcsetattr(stdin, termios.TCSADRAIN, saved)
        serve.server.close()
        os.unlink(path)
    _, status = os.waitpid(pid, 0)
    return os.waitstatus_to_exitcode(status)


if __name__ == "__main__":
    sys.exit(main())
