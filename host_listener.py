#!/usr/bin/env python3
"""Host-side listener for rc-notify events.

The transport depends on the host OS:

  Linux:  a FIFO at $XDG_RUNTIME_DIR/rc-notify.fifo (or /run/user/$UID/...)
  macOS:  a unix socket at ~/.local/share/containers/rc-notify.sock

Why two transports: launch.sh bind-mounts this endpoint into the container.
On some Linux kernels a process inside the container's mount/user namespace
cannot connect() to a bind-mounted unix socket even when ownership,
permissions and the LSM all allow it — but writing to a bind-mounted FIFO
works. macOS must stay a unix socket because the endpoint is shared into the
podman machine VM over virtiofs (Apple VZ default), and a FIFO's in-kernel
pipe buffer does not cross that host<->VM boundary; the macOS path is chosen
inside the home directory so virtiofs shares it into the VM at the same
absolute path, making the --volume mount work without extra configuration.

For each event it reads a single short line, validates it against a fixed
whitelist, and spawns host_notify.sh to render the desktop notification +
play the sound.

The endpoint file is the only thing launch.sh mounts into the container, so a
process in the container can deliver exactly these events and nothing else on
the host. The listener is the trust boundary — it rejects anything outside the
whitelist, so a compromised container cannot spoof arbitrary notification
bodies under the "Claude Code" brand.
"""

import os
import pathlib
import re
import signal
import socket
import subprocess
import sys

SCRIPT_DIR = pathlib.Path(__file__).resolve().parent
NOTIFY_SCRIPT = SCRIPT_DIR / "host_notify.sh"

# Must stay in sync with the case branches in host_notify.sh.
ALLOWED_EVENTS = {"done", "waiting"}

# Upper bound on bytes read per event.
READ_LIMIT = 128

# The label (which container fired the event) is the only attacker-influenceable
# text that reaches the desktop notification, so we strip it to a safe charset
# and cap its length before it ever leaves this trust boundary. This keeps a
# compromised container from smuggling arbitrary text under the "Claude Code"
# brand or injecting control characters.
_LABEL_STRIP = re.compile(r"[^A-Za-z0-9._-]")
LABEL_MAX = 40


def sanitize_label(label: str) -> str:
    return _LABEL_STRIP.sub("", label)[:LABEL_MAX]


def notify_target() -> "tuple[pathlib.Path, str]":
    """Return (path, kind) where kind is 'fifo' (Linux) or 'socket' (macOS)."""
    if sys.platform == "darwin":
        base = pathlib.Path.home() / ".local" / "share" / "containers"
        base.mkdir(parents=True, exist_ok=True)
        return base / "rc-notify.sock", "socket"
    runtime = os.environ.get("XDG_RUNTIME_DIR") or f"/run/user/{os.getuid()}"
    return pathlib.Path(runtime) / "rc-notify.fifo", "fifo"


def handle_event(raw: str) -> None:
    line = raw.strip()
    if not line:
        return
    # Wire format is "event" or "event<TAB>label".
    event, _, label = line.partition("\t")
    event = event.strip()
    if event not in ALLOWED_EVENTS:
        print(
            f"rc-notify: rejecting unknown event {event[:32]!r}",
            file=sys.stderr,
            flush=True,
        )
        return
    args = [str(NOTIFY_SCRIPT), event]
    label = sanitize_label(label.strip())
    if label:
        args.append(label)
    subprocess.Popen(
        args,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


def install_cleanup(path: pathlib.Path) -> None:
    def cleanup(*_args):
        try:
            path.unlink()
        except FileNotFoundError:
            pass
        sys.exit(0)

    signal.signal(signal.SIGINT, cleanup)
    signal.signal(signal.SIGTERM, cleanup)


def serve_fifo(path: pathlib.Path) -> None:
    if path.exists() or path.is_symlink():
        path.unlink()

    # Create 0600 (owner-only): the container writes as the mapped host user,
    # which owns the FIFO, so owner-write is sufficient.
    old_umask = os.umask(0o077)
    try:
        os.mkfifo(path, 0o600)
    finally:
        os.umask(old_umask)

    install_cleanup(path)
    print(f"rc-notify: reading FIFO {path}", flush=True)

    while True:
        # open() blocks until a writer connects; the loop then yields lines
        # until every writer closes (EOF), at which point we reopen and block
        # for the next event. Each readline is byte-bounded so a writer that
        # never sends a newline cannot make us buffer unboundedly.
        with open(path, "r") as fifo:
            for line in iter(lambda: fifo.readline(READ_LIMIT), ""):
                handle_event(line)


def serve_socket(path: pathlib.Path) -> None:
    if path.exists():
        path.unlink()

    # Create the socket with 0600 via umask: the socket file inherits
    # mode (0666 & ~umask). $XDG_RUNTIME_DIR is already 0700/user-only;
    # on macOS the containers directory is user-owned. Defence in depth.
    old_umask = os.umask(0o077)
    try:
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.bind(str(path))
    finally:
        os.umask(old_umask)
    sock.listen(8)

    install_cleanup(path)
    print(f"rc-notify: listening on {path}", flush=True)

    while True:
        conn, _ = sock.accept()
        try:
            data = conn.recv(READ_LIMIT)
        finally:
            conn.close()
        handle_event(data.decode("utf-8", errors="replace"))


def serve_tcp(host: str, port: int) -> None:
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    sock.bind((host, port))
    sock.listen(8)

    print(f"rc-notify: listening on tcp://{host}:{port}", flush=True)

    while True:
        conn, _ = sock.accept()
        try:
            data = conn.recv(READ_LIMIT)
        finally:
            conn.close()
        handle_event(data.decode("utf-8", errors="replace"))


def main() -> None:
    # Laptop side of the two-hop remote setup: listen on a loopback TCP port
    # that an SSH reverse tunnel feeds from the remote server, then render
    # locally. Same validation + host_notify.sh path as the local transports.
    #
    # Force RC_NO_FORWARD so the rendered child never re-forwards the event,
    # even if this checkout's .env happens to carry RC_FORWARD_ADDR.
    listen_tcp = os.environ.get("RC_LISTEN_TCP")
    if listen_tcp:
        os.environ["RC_NO_FORWARD"] = "1"
        host, sep, port = listen_tcp.rpartition(":")
        if not sep:
            host, port = "127.0.0.1", listen_tcp
        serve_tcp(host or "127.0.0.1", int(port))
        return

    path, kind = notify_target()
    if kind == "fifo":
        serve_fifo(path)
    else:
        serve_socket(path)


if __name__ == "__main__":
    main()
