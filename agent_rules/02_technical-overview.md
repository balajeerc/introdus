# Technical / architecture overview

## The shape

`introdus` is a Rust control plane that runs **on the container host** — where it
has full access to `.env`, `podman`, deploy keys, and `podman exec -u root`. It
drives everything through a persistent **tmux session** with a full-screen
**control TUI**. The **dev container itself runs no Rust**: its security core is
audited bash that `introdus` embeds and bind-mounts at launch.

One binary, three roles mirroring the three tiers:

- **Container host:** `introdus` (default = launch + control TUI),
  `introdus notify-host` (desktop render / forward / ntfy push).
- **Dev machine (laptop):** `introdus notify-listen` (+ ssh reverse tunnel).
- **Dev container:** no Rust — embedded bash entrypoint/setup + `rc-notify`.

## Cargo workspace

Two crates (`resolver = "2"`, edition 2021, `rust-version` 1.80):

- **`introdus-core`** — pure library: typed `.env` config, host paths, podman
  object naming, the embedded container-side bash assets, the agent registry,
  egress-allowlist logic, the notification trust boundary, and thin
  `podman`/`tmux`/process wrappers. Deps: `anyhow`, `dotenvy`, `dirs`.
- **`introdus-cli`** — the `introdus` binary: clap CLI, launch orchestration,
  the ratatui TUI (wizard + control panel), tmux session model, and the
  notification services. Deps: `clap`, `anyhow`, `dirs`, `ratatui` (crossterm
  backend re-exported by ratatui — no separate crossterm dep). Dev-dep:
  `rexpect` for pty integration tests.

## Launch flow

`introdus launch` (the default subcommand) runs, end to end:

1. **preflight** — Linux rootless podman only; check `podman` + `pasta` (+ tmux
   for the session model). Egress lives in the container, so the host needs
   nothing else.
2. **config** — load/parse `.env` into a typed `Config` (or run the **wizard**
   on first launch, writing `.env`).
3. **context** — resolve a `LaunchContext`: podman object names, a per-container
   assets dir (materialized bash core + build context), the generated proxy
   allowlist, and resolved cloudflared/paseo tunnel IPs.
4. **image** — build the shared `introdus-base:latest` when stale (staleness =
   the introdus binary is newer than the image, since assets are re-materialized
   each launch), tag a cheap per-project alias.
5. **lifecycle** — legacy cleanup; `--recreate` drops the container but keeps
   the volume; `--reset` also wipes the volume (guarded by a dirty-git scan +
   typed confirmation).
6. **run** — the full `podman run` flag/env/mount set, then hand off to the
   container. `--verify` runs a throwaway egress self-check; `--update` does an
   in-container refresh.

Everything then lives inside **one tmux session per container**
(`introdus-<adjective>-<adjective>-<noun>`, derived deterministically from the
project name and persisted as `SESSION_NAME`): two windows — `main-control` (the
control TUI) and `dev-container` (podman logs) — plus the `notify-host` service
running **detached, with no tmux window** (its output goes to a per-session log
the menu shows on demand), and on-demand `root-bash` / `dev-bash` / per-agent
windows.

## Inside the container (PID 1 = `firewall-entrypoint.sh`, root + CAP_NET_ADMIN)

1. Stage the deploy key into `dev`'s `~/.ssh` (while still root).
2. Install an nft **default-deny** egress filter, segregated by uid — only the
   proxy user (`rcproxy`) may reach the internet; DNS, loopback, and configured
   internal CIDRs are permitted.
3. Start the loopback hostname-allowlist proxy (tinyproxy) as `rcproxy`.
4. **Egress self-check**: canary IP direct-dial must fail, an allowlisted host
   must be reachable *through the proxy*, and a direct dial to that host's IP
   must fail (no IP bypass). Any failure aborts.
5. Drop all privilege and `exec` `/setup.sh` as non-root `dev` (which clones the
   repo, runs the optional launch hook, and starts the workload).

See [05_security.md](05_security.md) for the full threat model.

## Config and persistence

`.env` is the on-disk source of truth (typed `Config` ⇄ `.env` via `dotenvy`),
kept hand-editable; the wizard/TUI is the primary editor and normalizes the file
on save. Host-side generated artifacts (the bind-mounted proxy allowlist, the
materialized bash core) live under `$XDG_STATE_HOME/introdus`. Per-project data
(repo, toolchains, `node_modules`) persists in a podman volume across restarts.

## What stays bash, and why

The security-critical container core (`firewall-entrypoint.sh`, `tinyproxy.conf`,
`setup.sh`, `container/bin/*`, `container/agents.sh`) is **not** rewritten in
Rust — it is embedded via `include_str!`, materialized, and bind-mounted at
launch. This keeps the egress guarantee resting on the same battle-tested shell +
nft + tinyproxy, and lets edits to those files apply on a plain relaunch without
an image rebuild. The agent registry exists in two hand-synced copies:
`crates/introdus-core/src/agents.rs` (host-side, for the wizard/launch) and
`container/agents.sh` (in-container installer) — change both together.
