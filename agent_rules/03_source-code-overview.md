# Source-code overview

A concise map of the tree. Keep this current: when you add/move/rename a module
or change what one owns, update the matching line (per
[00_agent_instructions.md](00_agent_instructions.md)).

## `crates/introdus-core/` — pure library (no I/O orchestration)

| File          | Owns |
| ------------- | ---- |
| `lib.rs`      | Crate root; `pub mod` list, `VERSION`, `BIN_NAME`. |
| `config.rs`   | Typed project `Config` ⇄ `.env` round-trip (`load`/`render`/`save`), default whitelist. |
| `env_file.rs` | Low-level `.env` I/O: `dotenvy` read, list-splitting, value quoting. |
| `egress.rs`   | Pure allowlist logic: git-host extraction, per-host anchored regex, container whitelist assembly, tunnel edge IPs/hosts. |
| `agents.rs`   | The coding-agent registry (`AGENTS`) + install method / yolo-flag metadata; `paseo` orchestrator constants. Hand-mirrors `container/agents.sh`. |
| `assets.rs`   | The embedded container-side bash core (`include_str!`) and `materialize` into the per-container assets/build-context dir. |
| `names.rs`    | Podman object naming (base image, per-project image tag, container, volume); deterministic suffix fallback. |
| `paths.rs`    | Host state dir (`$XDG_STATE_HOME/introdus`) + generated-artifact paths (allowlist, notify log, launch marker, assets dir). |
| `ports.rs`    | Parse/validate `EXTRA_PORTS` entries. |
| `session.rs`  | Whimsical deterministic tmux session-name generation. |
| `notify.rs`   | The notification trust boundary: wire-format parse, event whitelist, label sanitization. |
| `podman.rs`   | Thin `podman` command constructors + existence/state probes. |
| `tmux.rs`     | Thin `tmux` helpers (sessions, windows, attach). |
| `process.rs`  | `Cmd` — the logged wrapper over `std::process::Command` all external tools go through; stdout capture guard for the TUI output pane. |

## `crates/introdus-cli/` — the `introdus` binary

| File             | Owns |
| ---------------- | ---- |
| `main.rs`        | clap CLI: `Command` enum → subcommand dispatch. |
| `wizard.rs`      | First-run setup wizard (inline ratatui modals) → writes `.env`. |
| `preflight.rs`   | Host checks: rootless podman + pasta (+ tmux for the session). |
| `context.rs`     | `LaunchContext` — everything the launch path derives from a `Config` (names, assets dir, allowlist, tunnel IPs). |
| `launch.rs`      | Top-level launch orchestration (preflight → image → lifecycle → run); `verify`/`update`/`rebuild-base`. |
| `image.rs`       | Base-image build/tag/prune; binary-newer-than-image staleness. |
| `lifecycle.rs`   | Container/volume lifecycle: cleanup, `--recreate`, `--reset` (dirty-git guard + typed confirm). |
| `run.rs`         | The full `podman run` flag/env/mount set; `--verify` self-check; `--update` in-container refresh. |
| `session.rs`     | The tmux session model — puts each container in one session with its windows. |
| `menu.rs`        | The control TUI (`introdus menu`): menu layout, dispatch to `menu_actions`. |
| `menu_actions.rs`| Implementations of each control-menu utility (tunnel URL, agents, allowlist, terminals, copy-in, ntfy, recreate/reset/stop/destroy, paseo). |
| `panel.rs`       | The persistent two-pane control panel (status+menu / streaming output), popup prompts. |
| `ui.rs`          | Shared ratatui primitives: status/row types, key reading, prompt state machines, the wizard's standalone inline modals. |
| `notify.rs`      | Host notification service: `notify-host` (FIFO/socket → ntfy/forward/desktop) and `notify-listen` (laptop TCP side). |
| `install.rs`     | `introdus install` — put the binary on `PATH`. |
| `util.rs`        | Small shared helpers (tilde expansion, shell quoting). |
| `tests/`         | pty integration tests (`wizard_pty.rs`, `menu_pty.rs`) + `common/`. See [06_testing.md](06_testing.md). |

## Embedded container-side bash (`container/`, `setup.sh`, `Dockerfile`)

Not compiled — embedded by `assets.rs` and bind-mounted at launch.

- `container/egress/firewall-entrypoint.sh` — PID 1: nft default-deny + tinyproxy
  + egress self-check, then drops privilege to `dev`.
- `container/egress/tinyproxy.conf` — the hostname-allowlist proxy config.
- `setup.sh` — post-firewall: clone repo, run launch hooks, start the workload.
- `container/agents.sh` — in-container agent-install registry (mirror of
  `agents.rs`).
- `container/bin/*` — `run-claude`, `install-agents`, `rc-notify` (container→host
  event sender), `egress-log`.
- `Dockerfile` — the base image; `COPY`s the `container/` tree.

## Tooling / meta

- `scripts/lint.sh`, `scripts/install-pre-commit.sh` — see [04_linting.md](04_linting.md).
- `scripts/gen-agent-rules.sh` — regenerate the root `AGENTS.md` (Codex/Pi/opencode)
  from `agent_rules/*.md`; `--check` mode gates drift in `lint.sh --quick`. See
  [00_agent_instructions.md](00_agent_instructions.md) for the rules-distribution setup.
- `test-harness/` — heavy nested-podman E2E suite — see [06_testing.md](06_testing.md).
- `deny.toml`, `clippy.toml`, `rustfmt.toml`, `Cargo.toml` `[workspace.lints]` —
  lint config.
- `PLAN.md` — rewrite design + milestone status. `TEST_PLAN.md` — test matrix.
