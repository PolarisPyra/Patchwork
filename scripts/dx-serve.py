#!/usr/bin/env python3
"""Runs `dx serve` in a pseudo-terminal, with function keys for its shortcuts and a compact log.

Usage (scripts/cargo-runner.sh runs it for `cargo run desktop`):
    uv run --no-project python3 scripts/dx-serve.py [DX ARGS...]

dx reads single letters from its terminal and draws a full-screen log of everything it does. This
script sits between the terminal and dx:

- Each save is patched into the open app as it lands. F5 applies changes by hand: it has dx
  patch every Rust file saved since the last F5, in case a save was missed.
- F7 pauses and resumes hot reload (dx's `p`). While it is paused dx ignores saves; F5 patches
  them into the open app and stays paused, and resuming patches them too.
- Shift+F5 rebuilds and restarts the app (dx's `r`), and F8 shows dx's debug lines too. Every
  other key, ctrl+c included, goes to dx as typed.
- dx's own screen is hidden. In its place each compiler error and warning, and each thing dx
  does, prints as one coloured line, with a status line under them. dx writes its log to a file
  this script follows, and the compiler's diagnostics as JSON among its output.
- The app drives the same shortcuts over a Unix socket whose path is in `PATCHWORK_DX_CONTROL`.
  It sends `apply`, `restart` or `toggle-paused`, one per line. On connecting and whenever they
  change it gets `paused` or `watching`, then `build starting`, `build building`, `build failed`
  or `build ready`; and `patched` each time dx patches the app.

dx patches only a file it sees change, so F5 rewrites each Rust file saved since the last F5
with the bytes it already holds; dx skips the ones it has already patched. While paused it
first resumes dx and waits for dx to say so, and pauses dx again once dx has taken them.
"""

from __future__ import annotations

import errno
import fcntl
import json
import os
import pty
import re
import select
import signal
import socket
import struct
import sys
import termios
import time
import tty
import unicodedata

# Shift+F5 and F7 as terminals send them, and the dx letter each one stands for.
KEYS = {
    b"\x1b[15;2~": b"r",
    b"\x1b[18~": b"p",
}
# F5 and F8, which this script handles itself.
APPLY = b"\x1b[15~"
VERBOSE = b"\x1b[19~"
SEQUENCES = sorted([*KEYS, APPLY, VERBOSE], key=len, reverse=True)

COMMANDS = {b"apply", b"restart", b"toggle-paused"}

# What dx logs when it resumes or pauses.
TOGGLED = b"Automatic rebuilds are currently"
# dx's screen asks where the cursor is before it draws; this script answers for the terminal.
CURSOR_QUERY = b"\x1b[6n"
# How long to wait for dx to say it resumed before rewriting the files anyway.
RESUME_WAIT = 1.0
# How long dx gets to take the rewritten files before it is paused again.
SETTLE = 1.0
# How often the log file is read and the status line's clock moves.
POLL = 0.1
# How long a message dx repeats is printed only once.
REPEAT = 3.0
# Folders that hold nothing the app builds from.
SKIP = {"target", "node_modules"}

# The start of a line of compiler JSON in dx's output.
JSON_START = re.compile(rb'\{"(?:reason|\$message_type)"')
# A record's first line in dx's log file; the lines after it carry on the same record.
RECORD = re.compile(r"^\[(TRACE|DEBUG|INFO|WARN|ERROR)\] (\S+): ?(.*)$")
# What dx logs once it has patched the app.
PATCHED = re.compile(r"^Hot-patching: (.+) took (\d+)ms")
# A compiler error's code; warnings carry the lint's name instead, which is left out.
ERROR_CODE = re.compile(r"^E\d+$")
# Compiler messages that only sum up the ones before them.
SUMMARY = re.compile(
    r"^(aborting due to|\d+ warnings? emitted|For more information|"
    r"Some errors have detailed explanations|could not compile)"
)
# What rustc warns each time dx replays a crate for a patch: dx reruns it with the jobserver
# variable cargo set for the first build, whose file descriptors are long closed.
JOBSERVER = "failed to connect to jobserver"

RESET, BOLD, DIM = "\x1b[0m", "\x1b[1m", "\x1b[2m"
RED, GREEN, YELLOW, CYAN = "\x1b[31m", "\x1b[32m", "\x1b[33m", "\x1b[36m"
# How each kind of line is labelled, and its colour.
LEVELS = {
    "error": ("error", RED),
    "warning": ("warn", YELLOW),
    "info": ("info", CYAN),
    "ok": ("ok", GREEN),
    "debug": ("debug", DIM),
}


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


def cells(text: str) -> int:
    """How many terminal columns `text` takes."""
    return sum(
        0 if unicodedata.combining(c) else 2 if unicodedata.east_asian_width(c) in "WF" else 1
        for c in text
    )


def clip(text: str, room: int) -> str:
    """`text` cut to fit `room` columns, ending in an ellipsis when it was cut."""
    if cells(text) <= room:
        return text
    out, used = [], 0
    for c in text:
        used += cells(c)
        if used > room - 1:
            break
        out.append(c)
    return "".join(out) + "…"


def one_line(text: str) -> str:
    return " ".join(text.split())


def seconds(elapsed: float) -> str:
    return f"{elapsed * 1000:.0f}ms" if elapsed < 1 else f"{elapsed:.1f}s"


class Screen:
    """The compact log: one line per message, and a status line kept under the last one."""

    def __init__(self, fd: int, terminal: bool) -> None:
        self.fd = fd
        self.terminal = terminal
        self.width = 100
        self.verbose = False
        self.drawn = ""
        # The last line printed and when, so a message dx repeats prints once.
        self.last: tuple[str, str, str] | None = None
        self.last_at = 0.0
        # Builds the status line, set by the owner.
        self.status = lambda: ""

    def resize(self, size: bytes | None) -> None:
        if size:
            self.width = max(struct.unpack("HHHH", size)[1], 20)
        self.drawn = ""
        self.refresh()

    def paint(self, text: str, *styles: str) -> str:
        return "".join(styles) + text + RESET if self.terminal and styles else text

    def entry(self, level: str, message: str, where: str = "") -> None:
        """Prints one line: the time, the level, the message and where it points."""
        now = time.monotonic()
        if self.last == (level, message, where) and now - self.last_at < REPEAT:
            return
        self.last, self.last_at = (level, message, where), now
        label, color = LEVELS[level]
        stamp = time.strftime("%H:%M:%S")
        message = one_line(message)
        head = cells(stamp) + 7
        room = self.width - head - (cells(where) + 2 if where else 0) - 1
        if room < 20:
            where, room = "", self.width - head - 1
        line = (
            self.paint(stamp, DIM)
            + " "
            + self.paint(f"{label:<5}", BOLD, color)
            + " "
            + clip(message, max(room, 1))
            + (" " + self.paint(" " + where, DIM) if where else "")
        )
        if self.terminal:
            write_all(self.fd, f"\r\x1b[2K{line}\r\n".encode())
            self.drawn = ""
            self.refresh()
        else:
            write_all(self.fd, f"{line}\n".encode())

    def refresh(self) -> None:
        """Draws the status line again when it changed."""
        if not self.terminal:
            return
        text = self.status()
        if text != self.drawn:
            self.drawn = text
            write_all(self.fd, f"\r\x1b[2K{text}".encode())

    def leave(self) -> None:
        if self.terminal:
            write_all(self.fd, b"\r\x1b[2K\x1b[?25h")


class Build:
    """What dx is building, from its log and the compiler's diagnostics."""

    def __init__(self, screen: Screen, root: str) -> None:
        self.screen = screen
        self.root = root
        self.state = "starting"
        self.started = time.monotonic()
        # How many times dx has patched the app, so the app can flash each patch.
        self.patches = 0
        self.crates = 0
        self.errors = 0
        self.warnings = 0
        # A failure waiting on the compiler errors logged under it.
        self.failure: str | None = None
        # The diagnostics shown for this build; dx prints each one twice.
        self.seen: set[tuple[str, str, str]] = set()

    def begin(self) -> None:
        if self.state != "building":
            self.started = time.monotonic()
            self.crates = self.errors = self.warnings = 0
            self.seen.clear()
        self.state = "building"

    def compiler(self, line: bytes) -> None:
        """Takes one line of cargo's or rustc's JSON."""
        try:
            item = json.loads(line)
        except ValueError:
            return
        if not isinstance(item, dict):
            return
        if item.get("reason") == "compiler-artifact":
            self.crates += 1
        elif item.get("reason") == "compiler-message":
            self.diagnostic(item.get("message") or {})
        elif item.get("$message_type") == "diagnostic":
            self.diagnostic(item)

    def diagnostic(self, item: dict) -> None:
        level = str(item.get("level", ""))
        if not (level.startswith("error") or level == "warning"):
            return
        message = one_line(str(item.get("message", "")))
        if message.startswith(JOBSERVER):
            return
        spans = item.get("spans") or []
        span = next((s for s in spans if s.get("is_primary")), spans[0] if spans else None)
        if span is None and SUMMARY.match(message):
            return
        where, label = "", ""
        if span:
            path = str(span.get("file_name", ""))
            if os.path.isabs(path) and path.startswith(self.root + os.sep):
                path = os.path.relpath(path, self.root)
            where = f"{path}:{span.get('line_start')}:{span.get('column_start')}"
            label = one_line(str(span.get("label") or ""))
        key = (level, message, where)
        if key in self.seen:
            return
        self.seen.add(key)
        code = str((item.get("code") or {}).get("code") or "")
        text = message + (f": {label}" if label and label not in message else "")
        if ERROR_CODE.match(code):
            text = f"{code} {text}"
        if level == "warning":
            self.warnings += 1
            self.screen.entry("warning", text, where)
        else:
            self.errors += 1
            self.screen.entry("error", text, where)

    def record(self, level: str, text: str) -> None:
        """Takes the first line of one of dx's log records."""
        text = one_line(text)
        if level == "TRACE":
            if text.startswith("Building with cargo"):
                self.begin()
            return
        if level == "DEBUG":
            if text.startswith("Patch rebuild"):
                self.begin()
            if self.screen.verbose:
                self.screen.entry("debug", text)
            return
        if not text.strip("-") or text.startswith(
            ("Automatic rebuilds are currently", "Verbose logging is now")
        ):
            return
        if patched := PATCHED.match(text):
            self.state = "ready"
            self.patches += 1
            took = seconds(int(patched[2]) / 1000)
            self.screen.entry("ok", f"Patched {patched[1]} in {took}{self.tally()}")
        elif text.startswith(("Build failed", "Failed to hot-patch")):
            # The compiler's errors for a patch come on the lines under this record, so the line
            # waits for them; see `settle`.
            self.state = "failed"
            self.failure = text
        elif text.startswith("Build completed"):
            self.state = "ready"
            first = "launching app" in text
            self.screen.entry(
                "ok",
                f"Built in {seconds(self.took())}{self.tally()}"
                + (", opening the app" if first else ""),
            )
        else:
            if text.startswith(("Full rebuild", "Starting full rebuild")):
                self.begin()
            self.screen.entry({"INFO": "info", "WARN": "warning", "ERROR": "error"}[level], text)

    def settle(self) -> None:
        """Prints the failure the last record held back, once its errors are in."""
        if self.failure is None:
            return
        text, self.failure = self.failure, None
        counts = self.tally().removeprefix(", ")
        if text.startswith("Build failed") and counts:
            text = f"Build failed: {counts}"
        self.screen.entry("error", text)

    def took(self) -> float:
        return time.monotonic() - self.started

    def tally(self) -> str:
        parts = [
            f"{n} {word}{'' if n == 1 else 's'}"
            for n, word in ((self.errors, "error"), (self.warnings, "warning"))
            if n
        ]
        return ", " + ", ".join(parts) if parts else ""


class Serve:
    def __init__(self, master: int, path: str, root: str, screen: Screen) -> None:
        self.master = master
        self.root = root
        self.screen = screen
        self.build = Build(screen, root)
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
        # The end of dx's output, to find `TOGGLED` and `CURSOR_QUERY` split across reads.
        self.tail = b""
        # dx's output since the last full line, while it may hold the start of compiler JSON.
        self.pending = b""
        # dx's last output, shown if it stops before drawing its screen.
        self.last = b""
        # Whether dx has started drawing its screen.
        self.drawing = False
        self.input = b""
        self.log: int | None = None
        self.log_pending = b""
        self.clients: dict[int, tuple[socket.socket, bytes]] = {}
        self.server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.server.bind(path)
        self.server.listen()
        self.server.setblocking(False)
        screen.status = self.status

    def status(self) -> str:
        """The status line: what dx is doing and the keys."""
        s = self.screen
        state = self.build.state
        if state == "building":
            crates = f", {self.build.crates} crates" if self.build.crates > 1 else ""
            what, color = f"● Building {self.build.took():.0f}s{crates}", CYAN
        elif state == "failed":
            what, color = "● Build failed", RED
        elif state == "starting":
            what, color = "● Starting dx", DIM
        elif self.paused:
            what, color = "● Paused", YELLOW
        else:
            what, color = "● Ready", GREEN
        keys = [
            ("F5", "apply"),
            ("F7", "resume" if self.paused else "pause"),
            ("Shift+F5", "restart"),
            ("F8", "quiet" if s.verbose else "verbose"),
            ("Ctrl+C", "quit"),
        ]
        plain = "  ".join(f"{k} {v}" for k, v in keys)
        if cells(what) + 3 + cells(plain) > s.width - 1:
            return s.paint(clip(what, s.width - 1), color)
        hints = "  ".join(s.paint(k, BOLD) + s.paint(f" {v}", DIM) for k, v in keys)
        return f"{s.paint(what, color)}   {hints}"

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
            applied = self.replay(repause=True)
        elif not self.dx_watching():
            self.to_rewrite = saved_since(self.root, self.since)
            applied = bool(self.to_rewrite)
            self.repause = False
            self.rewrite_all()
        else:
            return
        self.screen.entry("info", "Applying changes" if applied else "No changes to apply")

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
        self.screen.entry("info", "Hot reload paused" if self.paused else "Hot reload resumed")
        self.tell_all()

    def restart(self) -> None:
        write_all(self.master, b"r")
        # The rebuild takes every save, so none is left to apply.
        self.since = time.time()

    def toggle_verbose(self) -> None:
        self.screen.verbose = not self.screen.verbose
        self.screen.entry("info", f"Debug lines {'on' if self.screen.verbose else 'off'}")

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
        actions = {
            APPLY: self.apply,
            VERBOSE: self.toggle_verbose,
            b"p": self.toggle,
            b"r": self.restart,
        }
        plain = b""
        i = 0
        while i < len(data):
            sequence = next((s for s in SEQUENCES if data.startswith(s, i)), None)
            key = KEYS.get(sequence, sequence) if sequence else data[i : i + 1]
            i += len(sequence) if sequence else 1
            if key in actions:
                write_all(self.master, plain)
                plain = b""
                actions[key]()
            else:
                plain += key
        write_all(self.master, plain)

    def state(self) -> bytes:
        paused = b"paused\n" if self.paused else b"watching\n"
        return paused + f"build {self.build.state}\n".encode()

    def tell_all(self, extra: bytes = b"") -> None:
        for fd in list(self.clients):
            try:
                self.clients[fd][0].sendall(self.state() + extra)
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

    def from_dx(self, data: bytes) -> None:
        """Reads dx's output: answers its cursor queries and takes the compiler's JSON from it."""
        seen = self.tail + data
        if self.rewrite_at is not None and TOGGLED in seen:
            self.rewrite_all()
        for _ in range(seen.count(CURSOR_QUERY) - self.tail.count(CURSOR_QUERY)):
            self.drawing = True
            write_all(self.master, b"\x1b[1;1R")
        self.tail = seen[-len(TOGGLED) :]
        self.last = (self.last + data)[-4096:]
        *lines, self.pending = (self.pending + data).split(b"\n")
        for line in lines:
            if start := JSON_START.search(line):
                self.build.compiler(line[start.start() :].rstrip(b"\r"))
        start = JSON_START.search(self.pending)
        self.pending = self.pending[start.start() :] if start else self.pending[-32:]

    def follow_log(self, path: str) -> None:
        """Reads what dx added to its log file."""
        if self.log is None:
            try:
                self.log = os.open(path, os.O_RDONLY | os.O_NONBLOCK)
            except OSError:
                return
        while chunk := os.read(self.log, 65536):
            *lines, self.log_pending = (self.log_pending + chunk).split(b"\n")
            for line in lines:
                if match := RECORD.match(line.decode(errors="replace")):
                    self.build.settle()
                    before = (self.build.state, self.build.patches)
                    self.build.record(match[1], match[3])
                    if (self.build.state, self.build.patches) != before:
                        patched = self.build.patches != before[1]
                        self.tell_all(b"patched\n" if patched else b"")
                elif start := JSON_START.search(line):
                    # A failed patch logs the compiler's output under its record, not on screen.
                    self.build.compiler(line[start.start() :].rstrip(b"\r"))
        self.build.settle()


def main() -> int:
    stdin, stdout = sys.stdin.fileno(), sys.stdout.fileno()
    interactive = os.isatty(stdin)
    runtime = os.environ.get("XDG_RUNTIME_DIR") or "/tmp"
    path = os.path.join(runtime, f"patchwork-dx-{os.getpid()}.sock")
    log = os.path.join(runtime, f"patchwork-dx-{os.getpid()}.log")
    size = window_size(stdin) if interactive else None
    if os.path.exists(log):
        os.unlink(log)

    pid, master = pty.fork()
    if pid == 0:
        os.environ["PATCHWORK_DX_CONTROL"] = path
        args = ["serve", "--raw-json-diagnostics", "--log-to-file", log, *sys.argv[1:]]
        os.execvp("dx", ["dx", *args])

    if size:
        fcntl.ioctl(master, termios.TIOCSWINSZ, size)
    screen = Screen(stdout, os.isatty(stdout))

    def resize(*_: object) -> None:
        if size := window_size(stdin):
            fcntl.ioctl(master, termios.TIOCSWINSZ, size)
            os.kill(pid, signal.SIGWINCH)
            screen.resize(size)

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
    serve = Serve(master, path, os.getcwd(), screen)
    if screen.terminal:
        write_all(stdout, b"\x1b[?25l")
    screen.resize(window_size(stdout))
    try:
        while True:
            fds = [master, serve.server, *serve.clients]
            if interactive:
                fds.append(stdin)
            timeouts = [t for t in (serve.tick(),) if t is not None]
            timeouts.append(0.05 if serve.input else POLL)
            try:
                ready, _, _ = select.select(fds, [], [], min(timeouts))
            except InterruptedError:
                continue
            serve.follow_log(log)
            if not ready:
                serve.flush_input()
            if master in ready:
                try:
                    data = os.read(master, 65536)
                except OSError as e:
                    if e.errno != errno.EIO:
                        raise
                    data = b""
                if not data:
                    break
                serve.from_dx(data)
            if stdin in ready:
                serve.from_terminal(os.read(stdin, 1024))
            if serve.server in ready:
                serve.accept()
            for fd in ready:
                if fd in serve.clients:
                    serve.from_client(fd)
            screen.refresh()
    finally:
        screen.leave()
        if saved:
            termios.tcsetattr(stdin, termios.TCSADRAIN, saved)
        serve.server.close()
        os.unlink(path)
        if serve.log is not None:
            os.close(serve.log)
        if os.path.exists(log):
            os.unlink(log)
    _, status = os.waitpid(pid, 0)
    code = os.waitstatus_to_exitcode(status)
    if code and not serve.drawing:
        # dx stopped before it drew its screen, so what it printed is its error.
        write_all(stdout, serve.last)
    return code


if __name__ == "__main__":
    sys.exit(main())
