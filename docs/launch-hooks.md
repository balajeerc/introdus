# Launch hooks

> Part of [introdus](../README.md#features). Run your own setup on every container start — dev server, migrations, services.

Two optional hooks run on **every** container start, after the repo is cloned:
`ON_LAUNCH_ROOT_SCRIPT` (as root) then `ON_LAUNCH_SCRIPT` (as the non-root `dev`
user). Use them to bring up a dev server, run migrations, warm caches, or start a
system service the workload depends on.

## Prerequisites

None beyond a configured project. Both hooks run from the **repo root**. Each is
a full shell script, so multi-line blocks work.

## Usage

Set them in [config](setup-and-configuration.md#configuration-reference):

```bash
# Runs as the non-root `dev` user, from the repo root.
ON_LAUNCH_SCRIPT="
pnpm install
pnpm dev --host 0.0.0.0
"

# Runs AS ROOT, BEFORE ON_LAUNCH_SCRIPT — for root-needing setup only.
ON_LAUNCH_ROOT_SCRIPT="
./install_clickhouse_local.sh
clickhouse start --pid-path /var/run/clickhouse-server --config-path /etc/clickhouse-server
"
```

Notes that matter:

- **If a hook must be reachable from the host, bind to `0.0.0.0`** (not
  `127.0.0.1`) inside the container — the [published port](host-data-and-ports.md)
  forwards from the container's `0.0.0.0`. Also expose it via
  [`EXTRA_PORTS`](host-data-and-ports.md#extra-ports) or `WEBAPP_PORT`.
- **Blocking vs. backgrounding.** If a hook blocks (a foreground `pnpm dev`), the
  container stays alive on it. If it returns, the container falls through to an
  idle state. To background something, append `&` or wrap it in
  `tmux new-session -d -s NAME '...'`.
- **The root hook is the only way to do root-needing launch setup** — the dev
  workload is non-root and `no-new-privileges` blocks `sudo`. Use it for
  `apt install`, starting a system service, writing under `/etc` or `/var`.
- **Non-zero exits are logged but don't kill the container.** The root hook runs
  under a timeout (default 600s, override `ON_LAUNCH_ROOT_TIMEOUT`) so a *hung*
  one can't block startup — background long-running services so the script
  returns promptly.

Hook changes apply on a plain relaunch (no [recreate](persistence-and-lifecycle.md)
needed).

## How it works

Both hooks are executed by [setup.sh](../setup.sh) inside the container: the root
hook first (still privileged, under its timeout), then the dev workload is
`exec`'d and runs `ON_LAUNCH_SCRIPT`. This is after the
[egress filter and proxy](egress-filtering.md) are up, so any network the hook
does goes through the allowlist. Every step is idempotent, so a relaunch re-runs
the hooks cleanly.
