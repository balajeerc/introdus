# introdus — Rust rewrite of the remote-control-harness

> **Living document.** Milestones are checkboxes; tick them as they land.
> Commit freely to the `introdus-rust-rewrite` branch at each milestone.

## Context

`remote-control-harness` runs a dev environment (including AI coding agents
like Claude Code) inside a **network-hardened, rootless podman container**: a
default-deny nft egress filter plus a loopback hostname-allowlist proxy
(tinyproxy), with the workload running as a non-root `dev` user that has no
direct internet egress and cannot touch the filter. See `README.md` and
`docs/Technical details.md` for the current design.

The project has outgrown its original shape. Today it is a pile of bash scripts
(`launch_dev_container.sh`, `create-dev-container.sh`, `host_install.sh`, …), a
Python notification listener (`host_listener.py`), and a back-compat `launch.sh`
symlink. Configuration lives in a host-side `.env` that is **never mounted into
the container**, so any "post-start utility" that needs to persist config
cannot run from inside the container without punching a hole in the exact
isolation the project exists to provide.

**This rewrite** turns the whole *control plane* into a single self-contained
Rust binary (`introdus`) that runs **on the container host**, where it has full
access to `.env`, `podman`, deploy keys, and can `podman exec -u root`. It
drives everything through a persistent **tmux session** with a full-screen
**control TUI**, replacing both the CLI-arg soup and the `read`-based wizard.

The name **introdus** (Greg Egan, *Diaspora*): the transition of a mind from
flesh into a hardened digital substrate — here, moving your dev work into an
isolated cell.

## Locked design decisions

1. **Language:** Rust. One self-contained binary named `introdus`, placed on
   `PATH`. Kills the `launch.sh`-symlink / per-project-wrapper mechanism.
2. **Security core stays bash.** The audited container-side scripts
   (`firewall-entrypoint.sh`, `tinyproxy.conf`, `setup.sh`, `container/bin/*`)
   are **not** rewritten. The binary **embeds** them (`include_str!`) and
   materializes + bind-mounts them at launch, exactly as `launch.sh` does today.
   No Rust binary is compiled into the container image. This keeps the egress
   guarantee resting on the same battle-tested shell + nft + tinyproxy.
3. **Notifications fold into the binary.** `host_listener.py`,
   `host_notify.sh`, and `install_dev_machine_listener.sh` become `introdus`
   subcommands (no Python anywhere). The tiny in-container `rc-notify` sender
   stays bash (it runs as `dev` inside the container).
4. **Config format:** `.env` remains the on-disk source of truth (typed
   `Config` <-> `.env`), so existing projects keep working and the file stays
   hand-editable.
5. **Quality gates:** port syncer's extensive lint suite verbatim in spirit —
   `-D warnings` everywhere, rustfmt (max_width 100), clippy incl.
   `cognitive_complexity` ≤ 15, cargo-deny, cargo-audit, tokei 600-line
   per-file budget, **jscpd duplication detection**, semgrep. A tiered
   `scripts/lint.sh` + pre-commit hook.

## Target architecture

Single binary, three roles mirroring the project's three tiers:

- **Container host:** `introdus` (default = launch + control TUI),
  `introdus notify-host` (desktop render / forward / ntfy push).
- **Dev machine (laptop):** `introdus notify-listen` (+ ssh reverse tunnel).
- **Dev container:** *no Rust* — embedded bash entrypoint/setup + `rc-notify`.

### Cargo workspace

```
Cargo.toml                # [workspace] + [workspace.lints] + shared deps
crates/
  introdus-core/          # lib: config/.env, paths, agents registry,
                          #      embedded assets, podman/tmux/git wrappers,
                          #      allowlist gen, notify protocol
  introdus-cli/           # bin `introdus`: clap CLI + orchestration + notify +
                          #      the inquire-based wizard/control-menu TUI
                          #      (folded in rather than a separate crate)
container/                # UNCHANGED bash assets, embedded via include_str!
  egress/{firewall-entrypoint.sh,tinyproxy.conf}
  bin/{rc-notify,egress-log,run-agent,tunnel-url,install-agents}
  agents.sh               # retained as reference; data mirrored in Rust
Dockerfile                # base image (embedded / referenced by build)
scripts/lint.sh           # ported, Rust-only (no Kotlin/Android)
scripts/install-pre-commit.sh
tools/package.json        # jscpd via pnpm (local, no global pollution)
clippy.toml rustfmt.toml deny.toml .cargo/config.toml .cargo/audit.toml
```

### CLI surface (clap)

```
introdus                    # = `introdus launch` on the current project dir
introdus launch             # ensure tmux session, main-control TUI + container
introdus up                 # (internal) run/start the podman container (logs)
introdus menu               # (internal) the control TUI in the main-control pane
introdus verify             # egress smoke test (VERIFY_ONLY)
introdus recreate|reset     # rebuild container / wipe volume
introdus update             # in-container refresh (apt/mise/agents/lazyvim)
introdus rebuild-base       # rebuild the base image
introdus notify-host        # host notification service (was host_listener/notify)
introdus notify-listen      # laptop listener + reverse tunnel (was installer)
introdus install            # put binary on PATH + set up services (was host_install)
```

## tmux session model

`introdus launch`:
1. **Preflight** (hard-fail with install hints): `tmux`, `podman`, `pasta`.
   (TODO.md also wants `podman-compose`, `podman-toolbox` — check + warn.)
2. Resolve the **session name** (per container). First launch generates a
   whimsical `introdus-<adj>-<adj>-<noun>` and persists it in `.env`
   (`SESSION_NAME`).
3. Create/attach the tmux session; window 1 `main-control` runs `introdus menu`.
4. If no `.env`: the TUI runs the **wizard** first (writes `.env`, deploy key).
5. Window 2 `dev-container` runs `introdus up` (container logs stream here).
6. Utilities that open shells spawn new windows: `root-bash`
   (`podman exec -it -u root`), `dev-bash` (`podman exec -it --user dev`),
   and per-agent windows for launched agents.

## Control TUI utilities

| Utility | Mechanism | Persists to `.env`? |
| --- | --- | --- |
| Expose webapp (if not already) | toggle `EXPOSE_WEBAPP`; needs `--recreate` to apply | yes |
| Enable ntfy.sh + topic | `ENABLE_NOTIFY_SH_ALERTS`/`NTFY_SH_TOPIC` | yes |
| Show tunnel URL | read `~/.logs/tunnel-url.txt` (via `tunnel-url`) | n/a |
| Host file picker → shared dir | TUI file browser; copy into `SHARED_DATA_PATH` | maybe |
| Install an agent after the fact | append `INSTALL_AGENTS` + agent hosts to whitelist; run `install-agents` in the running container | yes |
| Launch an installed agent in tmux | generalized `run-agent` (Claude remote-control on) | no |
| List recently blocked egress URLs | parse tinyproxy log (`egress-log`) | n/a |
| Add URLs to allowlist | append `WHITELIST_HOSTS`; regen allowlist file; offer restart | yes |
| Open root terminal | new tmux window `root-bash` | no |
| Open dev terminal | new tmux window `dev-bash` | no |
| Test host notification | trigger `rc-notify` in container; observe on host/laptop | no |

## Milestones

- [x] **M0 — Scaffold + quality gates.** Cargo workspace (`introdus-core`,
      `introdus-cli` → binary `introdus`); ported lint config
      (rustfmt/clippy/deny/audit/`.cargo/config.toml`/workspace.lints), `tools/`
      jscpd, adapted `scripts/lint.sh` + `install-pre-commit.sh`. Binary
      compiles; `scripts/lint.sh --full` green (7/7). CLI subcommand skeleton
      stubbed. (Note: local `semgrep` install is broken — `pipx reinstall
      semgrep` to green the `--security` tier.)
- [x] **M1 — Core config & paths.** Typed `Config` <-> `.env` round-trip
      (`config.rs`, lossless, verified by test); `.env` I/O + quoting
      (`env_file.rs`, via `dotenvy`); XDG state/allowlist/assets paths
      (`paths.rs`); podman object naming + suffix hash (`names.rs`); agents
      registry ported to Rust (`agents.rs`). 11 unit tests green; lint --full
      green. New `SESSION_NAME` field added for the tmux model.
- [x] **M2 — Embedded assets + process wrappers.** `process.rs` (`Cmd`:
      logged spawn, stdout capture, exit-code mapping, unix `exec`);
      `podman.rs` + `tmux.rs` thin wrappers. `assets.rs` embeds the 11-file
      bash core via `include_str!` and materializes it (preserving the repo
      layout so the Dockerfile's `COPY`s resolve) into a per-container assets
      dir that doubles as build context + runtime bind-mount source. 20 tests
      green; lint --full green.
- [x] **M3 — Launch orchestration (`up`).** Ported `launch_dev_container.sh`:
      `preflight.rs` (linux/non-root/podman/pasta/rootless), `image.rs`
      (base build/tag/stale-prune, staleness keyed on binary mtime),
      `egress.rs` (git-host + allowlist regex escaping, tested against the
      shell's `sed`), `context.rs` (names/assets/allowlist/tunnel-IP resolve),
      `run.rs` (full `podman run` flag/env/mount set + `verify` + `update`),
      `lifecycle.rs` (legacy/recreate/reset with the dirty-git scan + typed
      confirm), `launch.rs` (end-to-end flow). CLI dispatches
      launch/up/recreate/reset/verify/update/rebuild-base. 26 tests; lint
      --full green. Deferred: `/run/notify` mount → M7; tmux-wrapping of
      `launch` → M4. (Not yet run against a real podman build — user does
      end-to-end at the end.)
- [x] **M4 — tmux session + `launch`.** `session.rs` (core): whimsical
      `introdus-<adj>-<adj>-<noun>` names, deterministic per project, persisted
      as `SESSION_NAME`. `tmux.rs` gained cwd-aware window helpers. CLI
      `session.rs`: `launch` mints/persists the name, then creates a detached
      session with `main-control` (runs `introdus menu`) + `dev-container`
      (runs `introdus up`) windows and attaches. `Up` is now the in-window
      container runner. 29 tests; lint --full green. (Wizard-on-missing-.env
      hook lands in M5; `menu` TUI in M6.)
- [x] **M5 — TUI wizard.** `wizard.rs` (built on `inquire`): guided `.env`
      creation — project name, repo URL, deploy-key path (offers ed25519
      generation + prints the pubkey to register), webapp port, agent
      multi-select checklist (extends the whitelist with each agent's egress
      hosts), tunnel + ntfy toggles. Wired into `session::launch` so a project
      with no `.env` runs the wizard, then launches. 29 tests; lint --full
      green. (Chose `inquire` over a from-scratch ratatui form engine —
      robust, small, works in tmux/SSH. TUI deps swapped in Cargo.toml.)
- [x] **M6 — TUI control panel + utilities.** `menu.rs` (inquire `Select`
      loop + status header) dispatching to `menu_actions.rs`: show tunnel URL,
      toggle expose-webapp, enable ntfy, copy a host file into the container,
      install an agent (runtime + persist + whitelist), launch an agent in a
      tmux window, list blocked egress, add allowlist hosts (+regen +restart),
      open root/dev terminals (new tmux windows), test notification, recreate,
      reset. Shared `util.rs` (tilde/shell-quote). Wired to `introdus menu`.
      29 tests; lint --full green.
- [x] **M7 — Notifications folded in.** `notify.rs` (core): the trust boundary
      — event whitelist + label sanitization ported exactly, tested. `notify.rs`
      (cli): `notify-host` serves the FIFO (Linux) / socket path and renders
      (ntfy push → TCP forward → desktop notify-send+sound / osascript+afplay),
      embedding the wav; `notify-listen` is the laptop TCP side. `launch` now
      mounts `/run/notify` and starts a `notify` tmux window with the project's
      notify env. In-container `rc-notify` stays bash. 32 tests; lint --full
      green. (ssh reverse-tunnel + systemd-unit installer → M8.)
- [x] **M8 — Host install / distribution.** `install.rs`: `introdus install`
      copies the running binary to `~/.local/bin` (XDG exec dir) + PATH
      guidance — replacing `host_install.sh`'s symlink step. The notify
      listener is per-session (tmux `notify` window), so no global systemd
      service is needed for the local case. Release profile confirmed: a single
      self-contained ~1.5M stripped binary (`cargo build --release`). All 11
      subcommands wired (no stubs). 32 tests; lint --full green. (ssh
      reverse-tunnel laptop-listener install remains a documented manual step
      using `introdus notify-listen`.)
- [ ] **M9 — Docs + cleanup.** README/docs/sample.env regenerated; retire old
      bash scripts (or thin shims); TODO. **Commit.**
- [ ] **M10 — Final lint/security pass + end-to-end verification notes.**

## Quality / lint setup (ported from `syncer`)

- `.cargo/config.toml`: `rustflags = ["-D", "warnings"]` (warnings are errors
  for every cargo invocation).
- `rustfmt.toml`: edition 2021, `max_width = 100`, Unix newlines, 4-space tabs.
- `clippy.toml`: `cognitive-complexity-threshold = 15`; enabled via
  `[workspace.lints.clippy] cognitive_complexity = "warn"`.
- `deny.toml` + `.cargo/audit.toml`: advisories/licenses/bans/sources, with a
  fresh (project-specific) ignore list — start empty, justify each addition.
- `tools/` jscpd (pnpm-local) for **cross-file duplication** — any clone fails
  the gate (`--exitCode 1`).
- `scripts/lint.sh` tiers: `--quick` (fmt+clippy) ⊂ `--full` (+tokei 600-line
  budget, cargo-machete, cargo-deny, cargo-audit, jscpd) ⊂ `--security`
  (+semgrep `p/rust`). Missing tools are recorded as failures, never skipped.
- `scripts/install-pre-commit.sh`: installs a `--security` pre-commit hook.

## Verification

- `scripts/lint.sh --full` (and `--security`) green at every milestone.
- `cargo test --workspace` for `introdus-core` logic (env round-trip, allowlist
  regex generation matching the current `sed`, agents registry, session-name
  generation).
- **End-to-end (manual, by the user at the end):** `introdus` on a real project
  dir → wizard → container comes up in `dev-container` window → egress
  self-check passes → each utility exercised (install agent, add allowlist host,
  root/dev terminals, tunnel URL, test notification).
- Egress parity is the non-negotiable invariant: the generated allowlist regexes
  and `podman run` flag/env set must match `launch_dev_container.sh` byte-for-
  meaning. Cross-check against the existing script during M3.
