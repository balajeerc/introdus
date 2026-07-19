# Host data & extra ports

> Part of [introdus](../README.md#features). Feed a read-only data dir in, and publish extra container ports to your local tools.

Two small host↔container plumbing features: mount a host directory the container
can **read**, and publish **extra ports** to `127.0.0.1` for local tools.

## Sharing host data (read-only)

### Prerequisites

An absolute path on the container host to the directory you want to share.

### Usage

Set `SHARED_DATA_PATH` in
[config](setup-and-configuration.md#configuration-reference):

```bash
SHARED_DATA_PATH=/home/you/data/my-project-inputs
```

introdus mounts it **read-only** at `/home/dev/shared_data` inside the container.
Use it for datasets, reference material, or anything the container should read but
never modify. Leave it unset to skip the mount. (To push files *in* that the
container can write, use [`send-files`](send-files.md) instead.)

## Extra ports

### Prerequisites

The service must listen on `0.0.0.0` inside the container (a
[launch hook](launch-hooks.md) that binds to `127.0.0.1` won't be reachable from
the host).

### Usage

Set `EXTRA_PORTS` — each entry is a single port (published host:container on the
same number) or `host:container` to remap when the host port is busy. Whitespace
or newlines separate entries:

```bash
EXTRA_PORTS="
8123
9000
16379:6379
"
```

All bindings go to **`127.0.0.1` only** — these are for local tools on this
machine (DBeaver against an in-container ClickHouse, a debugger attaching,
`redis-cli`), **not** LAN exposure. To reach them from a remote host's laptop,
[tunnel over SSH](remote-host.md#reaching-the-published-ports); for a public
webapp URL, use the [webapp tunnel](webapp-tunnel.md).

> `EXTRA_PORTS` is applied at container-create time — changing it needs an
> [`introdus recreate`](persistence-and-lifecycle.md).

## How it works

`EXTRA_PORTS` parsing/validation is in
[ports.rs](../crates/introdus-core/src/ports.rs); both the mount and the port
publishes are assembled into the `podman run` flag set in
[run.rs](../crates/introdus-cli/src/run.rs). All published ports (including
`WEBAPP_PORT`) bind to `127.0.0.1` as part of the
[hardened posture](container-hardening.md#runtime-posture).
