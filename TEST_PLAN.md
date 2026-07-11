# introdus — Test Plan

A feature-by-feature test catalogue for the `introdus` Rust control plane.
Consult it when validating a feature. Each case notes whether an **automated
test** covers it and a **manual-reliance rating (0–5)**.

## Manual-reliance rating (0–5)

How much the case *depends on a human running it in a real environment* — the
inverse of how much automation can prove it.

| Score | Meaning |
| :---: | ------- |
| **0** | Fully proven by automated tests. No manual testing needed. |
| **1** | Logic is automated; a glance is reassuring but optional. |
| **2** | Partly automated; manually check the integration edges. |
| **3** | Core logic automated, but the *observable* behaviour needs a manual run to trust. |
| **4** | Largely manual — automation only touches helpers; real behaviour must be observed. |
| **5** | Entirely manual — needs a live environment (podman / tmux / desktop / phone / network) and human eyes; no meaningful automation possible. |

**Automated** column legend: ✅ = unit test named; ⚠️ = partial (helper logic
only); ❌ = none.

Run the automated suite with `cargo test --workspace` and the quality gates with
`scripts/lint.sh --full` (or `--security`, which also runs semgrep).

The interactive `inquire` TUI is covered by **pty integration tests** under
`crates/introdus-cli/tests/` (`wizard_pty.rs`, `menu_pty.rs`), which spawn the
real binary through a pseudo-terminal via `rexpect` and drive the prompts as a
user would. They need no podman/tmux (the wizard is reached through the
standalone `introdus init`), so they run anywhere `cargo test` does.

---

## 1. Build & quality gates (M0)

| # | Test case | Automated | Manual | How to verify |
|---|-----------|:---------:|:------:|---------------|
| 1.1 | Workspace compiles (debug + release) | ⚠️ CI-of-sorts via `cargo test` | 1 | `cargo build --workspace && cargo build --release` |
| 1.2 | `scripts/lint.sh --full` passes (fmt, clippy, deny, audit, machete, tokei, jscpd) | ✅ the gate itself | 0 | `./scripts/lint.sh --full` |
| 1.3 | `scripts/lint.sh --security` passes (adds semgrep) | ✅ the gate | 1 | needs a working `semgrep` (`pipx reinstall semgrep` if broken) |
| 1.4 | Pre-commit hook installs and blocks a dirty commit | ❌ | 2 | `./scripts/install-pre-commit.sh`; try committing a fmt violation |
| 1.5 | Release binary is a single self-contained artifact | ❌ | 1 | `ldd target/release/introdus`; run it on a clean box |

## 2. Config & `.env` round-trip (M1 — `config.rs`, `env_file.rs`)

| # | Test case | Automated | Manual | How to verify |
|---|-----------|:---------:|:------:|---------------|
| 2.1 | `render` → `load` is lossless for a fully-populated config | ✅ `round_trip_preserves_config` | 0 | — |
| 2.2 | Defaults applied for a minimal `.env` (agents, whitelist, mem, pids, timeout, canary) | ✅ `defaults_applied_for_minimal_env` | 0 | — |
| 2.3 | Missing required field errors (`REPO_URL`, etc.) | ✅ `missing_required_field_errors` | 0 | — |
| 2.4 | Multi-line `WHITELIST_HOSTS` / `ON_LAUNCH_SCRIPT` parse (bash-quoted) | ✅ via 2.1 + `split_list…` | 1 | hand-write a multi-line `.env`, `introdus verify` reads it |
| 2.5 | Value quoting escapes `"`, `\`, `$`, backtick correctly | ✅ `quote_scalar_bare_vs_quoted` | 0 | — |
| 2.6 | An existing hand-written `.env` (from the bash flow) loads unchanged in meaning | ⚠️ | 2 | load a real project `.env`, diff `render` output for surprises |
| 2.7 | Saving normalizes/rewrites the file (comments regenerated) | ⚠️ | 2 | run a menu action that saves; inspect the `.env` |

## 3. Agent registry (M1 — `agents.rs`)

| # | Test case | Automated | Manual | How to verify |
|---|-----------|:---------:|:------:|---------------|
| 3.1 | Ids unique; claude prebaked; codex not | ✅ `ids_are_unique_and_claude_is_prebaked` | 0 | — |
| 3.2 | Script-method agents use URL specs | ✅ `script_agents_use_url_specs` | 0 | — |
| 3.3 | Registry stays in sync with `container/agents.sh` | ❌ (hand-kept) | 3 | diff the two by eye when either changes |
| 3.4 | Each agent's egress hosts are actually sufficient to auth | ❌ | 5 | install the agent, sign in, watch `egress-log` for blocks |

## 4. Naming & paths (M1 — `names.rs`, `paths.rs`)

| # | Test case | Automated | Manual | How to verify |
|---|-----------|:---------:|:------:|---------------|
| 4.1 | Container/volume/image names carry the suffix | ✅ `names_carry_suffix` | 0 | — |
| 4.2 | Image slug sanitizes uppercase/space/punctuation | ✅ `slug_sanitizes` | 0 | — |
| 4.3 | Fallback suffix deterministic, 4 hex, differs per host | ✅ `fallback_suffix_is_deterministic_and_4_hex` | 0 | — |
| 4.4 | State/allowlist path under `$XDG_STATE_HOME/introdus` | ✅ `allowlist_path_is_under_state_dir` | 0 | — |

## 5. Embedded assets (M2 — `assets.rs`)

| # | Test case | Automated | Manual | How to verify |
|---|-----------|:---------:|:------:|---------------|
| 5.1 | All 11 assets embedded, non-empty; entrypoint contains `nft` | ✅ `assets_embed_nonempty` | 0 | — |
| 5.2 | Materialize writes the tree with correct exec/non-exec modes | ✅ `materialize_writes_tree_with_modes` | 0 | — |
| 5.3 | Materialized build context actually `podman build`s | ❌ | 5 | `introdus rebuild-base` on a podman host |
| 5.4 | Embedded bash byte-identical to `container/` sources | ⚠️ (include_str! guarantees) | 1 | `git diff` shows no drift; rebuild after edits |

## 6. Process / podman / tmux wrappers (M2)

| # | Test case | Automated | Manual | How to verify |
|---|-----------|:---------:|:------:|---------------|
| 6.1 | `Cmd` arg/label building, exit-code mapping, stdout capture, ok-probe | ✅ `process::tests::*` | 0 | — |
| 6.2 | `podman exec` / `exec -it` flag building (`--user`) | ✅ `exec_builds_user_flag`, `interactive_exec_is_it` | 0 | — |
| 6.3 | `tmux attach` label | ✅ `attach_label` | 0 | — |
| 6.4 | The wrappers drive real podman/tmux correctly | ❌ | 4 | exercised implicitly by every live launch/menu action |

## 7. Preflight checks (M3 — `preflight.rs`)

| # | Test case | Automated | Manual | How to verify |
|---|-----------|:---------:|:------:|---------------|
| 7.1 | Errors on non-Linux / root / missing podman / missing pasta / non-rootless | ❌ | 3 | temporarily rename `pasta`; run `introdus up`; expect a clear error |
| 7.2 | `check_session` additionally requires tmux | ❌ | 3 | rename `tmux`; run `introdus`; expect the tmux hint |
| 7.3 | Passes cleanly on a correct host | ⚠️ | 2 | `introdus up` gets past preflight into `.env`/wizard logic |

## 8. Base image build / tag / prune (M3 — `image.rs`)

| # | Test case | Automated | Manual | How to verify |
|---|-----------|:---------:|:------:|---------------|
| 8.1 | Stale project-tag matcher (`introdus-<slug>-XXXX:latest`) | ✅ `stale_tag_matching` | 0 | — |
| 8.2 | Builds the base image when missing | ❌ | 5 | first `introdus up` on a clean host |
| 8.3 | Cached rebuild when the binary is newer than the image | ❌ | 4 | rebuild introdus, relaunch, watch for the "cached rebuild" line |
| 8.4 | `rebuild-base` forces `--no-cache` | ❌ | 4 | `introdus rebuild-base`; confirm layers rebuild |
| 8.5 | Per-project tag applied; stale suffixed tags pruned | ❌ | 3 | change `IMAGE_SUFFIX`, relaunch, `podman image ls` |

## 9. Egress allowlist generation (M3 — `egress.rs`)  ← security-critical

| # | Test case | Automated | Manual | How to verify |
|---|-----------|:---------:|:------:|---------------|
| 9.1 | Git-host extraction across `git@`/`ssh://`/`https://`/bare forms | ✅ `git_host_forms` | 0 | — |
| 9.2 | Allowlist regex escaping matches the shell's `sed` | ✅ `pattern_matches_shell_escaping` | 0 | — |
| 9.3 | Ordered whitelist = git host + WHITELIST + tunnel host | ✅ `container_whitelist_order_and_tunnel` | 1 | diff generated allowlist file vs a `./launch.sh` run |
| 9.4 | Rendered allowlist file = one pattern per line | ✅ `render_is_one_pattern_per_line` | 0 | — |
| 9.5 | **Proxy actually enforces the allowlist in the container** | ❌ | 5 | in-container: `curl allowed-host` ✓, `curl blocked-host` ✗, `egress-log` shows the block |

## 10. Container create — `podman run` flag set (M3 — `run.rs`)  ← security-critical

| # | Test case | Automated | Manual | How to verify |
|---|-----------|:---------:|:------:|---------------|
| 10.1 | Hardening flags present (`--cap-drop=ALL`, `no-new-privileges`, `pasta`, image→entrypoint) | ✅ `run_args_have_the_hardening_flags` | 1 | `podman inspect` the running container |
| 10.2 | `--disable-network-block` drops `NET_ADMIN` and sets the env | ✅ `disable_network_block_drops_net_admin` | 2 | launch with the flag; confirm unfiltered egress |
| 10.3 | Webapp + extra ports published to 127.0.0.1 | ✅ `publishes_webapp_and_extra_ports` | 1 | `podman port`, hit the port from host |
| 10.4 | Extra-port parse/validate (bad, out-of-range, collision) | ✅ `ports::tests::*` | 0 | — |
| 10.5 | All five bind-mounts + `/run/notify` + shared-data present | ⚠️ (built in 10.1) | 2 | `podman inspect` mounts on a live container |
| 10.6 | Deploy-key / shared-data existence validation | ⚠️ `validate_inputs` (no test) | 2 | point `.env` at a missing key; expect a clear error |
| 10.7 | Container actually boots, entrypoint drops to `dev` | ❌ | 5 | `podman exec ... id` shows uid 1000; egress self-check passes |

## 11. Egress self-check — `introdus verify` (M3)

| # | Test case | Automated | Manual | How to verify |
|---|-----------|:---------:|:------:|---------------|
| 11.1 | Canary blocked, proxy reaches allowlisted host, direct-IP blocked | ❌ | 5 | `introdus verify` → "verify passed" on a podman host |
| 11.2 | Verify aborts the launch on any failure | ❌ | 4 | remove the git host from WHITELIST; expect failure |

## 12. In-container update — `introdus update` (M3)

| # | Test case | Automated | Manual | How to verify |
|---|-----------|:---------:|:------:|---------------|
| 12.1 | Errors if the container isn't running | ⚠️ | 2 | run against a stopped container |
| 12.2 | apt/mise/claude/agents/lazyvim refresh runs through the proxy | ❌ | 5 | `introdus update`; watch it complete without egress blocks |

## 13. Lifecycle — recreate / reset / pull (M3, M6 — `lifecycle.rs`)

| # | Test case | Automated | Manual | How to verify |
|---|-----------|:---------:|:------:|---------------|
| 13.1 | Legacy pre-suffix container removed | ❌ | 3 | create a legacy-named container; relaunch |
| 13.2 | Recreate drops container, keeps volume | ❌ | 4 | recreate; confirm `/home/dev` (repo) survives |
| 13.3 | Reset wipes the volume | ❌ | 4 | reset; confirm the volume is gone |
| 13.4 | Reset scan detects **unstaged working-tree changes** | ❌ | 5 | edit a tracked file (no `git add`); reset; scan lists it under "working tree" |
| 13.5 | Reset scan detects **staged-but-uncommitted changes** | ❌ | 5 | `git add` a change; reset; scan lists it (`git status --porcelain` shows both) |
| 13.6 | Reset scan detects **untracked files** | ❌ | 4 | create a new untracked file; reset; scan lists it (`??`) |
| 13.7 | Reset scan detects **unpushed commits** (not on any remote) | ❌ | 5 | commit locally, don't push; reset; scan reports "unpushed commits: N" |
| 13.8 | Reset scan detects **stashes** | ❌ | 4 | `git stash`; reset; scan lists the stash |
| 13.9 | Scan walks **every repo** under `/home/dev/work` (multi-repo) | ❌ | 4 | dirty two repos; reset; both appear in the report |
| 13.10 | Typed `yes` confirmation is **always required**, even when the scan finds nothing / base image missing | ❌ | 5 | reset a clean volume; confirm it still demands `yes`; non-`yes` aborts with the volume intact |
| 13.11 | Scan is read-only and non-fatal (best-effort; failure never blocks the confirm) | ❌ | 4 | reset with an odd/corrupt repo; confirm the flow still reaches the `yes` prompt |
| 13.12 | `--pull` sentinel triggers a ff-only pull on next start | ❌ | 4 | `introdus up --pull`; confirm the repo fast-forwards |

## 14. tmux session model — `introdus launch` (M4 — `session.rs`)

| # | Test case | Automated | Manual | How to verify |
|---|-----------|:---------:|:------:|---------------|
| 14.1 | `window_cmd` builds `exec '<bin>' <sub>` | ✅ `window_cmd_execs_binary` | 0 | — |
| 14.2 | Session name minted + persisted to `.env` on first launch | ⚠️ (generator tested) | 3 | first `introdus`; grep `SESSION_NAME` in `.env` |
| 14.3 | Session created with main-control + notify + dev-container windows | ❌ | 4 | `introdus`; `tmux list-windows` shows all three |
| 14.4 | Re-launch re-attaches instead of spawning a duplicate | ❌ | 3 | run `introdus` twice |
| 14.5 | Wizard runs when `.env` is absent, then launches | ❌ | 4 | `introdus` in an empty dir |

## 15. Session naming (M4 — `session.rs` core)

| # | Test case | Automated | Manual | How to verify |
|---|-----------|:---------:|:------:|---------------|
| 15.1 | Deterministic per project, `introdus-adj-adj-noun` shape | ✅ `deterministic_and_shaped` | 0 | — |
| 15.2 | Two adjectives differ; distinct across projects | ✅ `adjectives_differ`, `differs_between_projects` | 0 | — |

## 16. Setup wizard (M5 — `wizard.rs`)

| # | Test case | Automated | Manual | How to verify |
|---|-----------|:---------:|:------:|---------------|
| 16.1 | Selected agents' egress hosts appended to whitelist | ✅ `apply_agents_extends_whitelist` | 0 | — |
| 16.2 | Prompts: name/repo/port/agents/tunnel/ntfy flow end-to-end | ✅ `wizard_pty::*` (pty-driven via `introdus init`) | 1 | `cargo test --test wizard_pty`; or walk it live |
| 16.3a | Deploy key — "generate new?" asked first; **yes** → prompts *where to create* (default `~/.ssh/introdus-deploy-keys/<slug>-deploy-key`, dir chmod 700), writes the keypair, prints the `.pub`, refuses to overwrite an existing file | ✅ `wizard_generates_a_new_key` (+ tilde/slug units) | 2 | pty test covers the happy path; manual for chmod 700 + overwrite-refusal |
| 16.3b | Deploy key — **no** → offers a project-matching key to reuse (yes/no; picker when several), else prompts for an existing path; registration step shown either way | ✅ `wizard_reuses_a_matching_key_and_still_shows_registration` | 2 | pty test covers reuse; manual for the bad-path re-ask |
| 16.4 | Wizard writes a valid, loadable `.env` | ✅ `wizard_pty::*` assert `.env` contents (+ round-trip unit) | 1 | pty tests read back the written `.env` |
| 16.5 | Cancel (Esc/Ctrl-C) aborts cleanly | ❌ | 3 | Esc mid-wizard |
| 16.6 | `introdus init` runs the wizard standalone (no podman); confirms before overwriting an existing `.env` | ✅ (pty tests invoke `init`) | 2 | `cargo test --test wizard_pty`; manual for the overwrite confirm |

## 17. Control TUI + utilities (M6 — `menu.rs`, `menu_actions.rs`)

| # | Test case | Automated | Manual | How to verify |
|---|-----------|:---------:|:------:|---------------|
| 17.1 | Menu loop renders status header, dispatches, survives action errors | ⚠️ `menu_pty` (renders, groups, filters, quits) | 3 | `cargo test --test menu_pty`; live for dispatch/error paths |
| 17.1a | Absent container reads as "not created" — no leaked `Error: no such container` | ✅ `menu_reports_not_created_without_leaking_podman_error` | 0 | pty regression test |
| 17.1b | Grouped sections render (inert headers) and the whole menu shows at once | ✅ (asserted in `menu_pty`) | 1 | pty test asserts a section header; eyeball the full layout |
| 17.2 | Show tunnel URL | ❌ | 5 | with `EXPOSE_WEBAPP`, menu → tunnel URL prints the trycloudflare URL |
| 17.3 | Toggle expose-webapp (persist + offer recreate) | ❌ | 4 | toggle; grep `.env`; recreate; confirm tunnel starts |
| 17.4 | Enable ntfy (topic prompt + persist) | ❌ | 4 | enable; grep `.env`; recreate; check phone |
| 17.5 | Copy a host file/folder into the container | ❌ | 4 | copy; `podman exec ... ls /home/dev/uploads` |
| 17.6 | Install an agent at runtime (persist + whitelist + run install-agents) | ❌ | 5 | install codex; confirm `.env`, whitelist, and the binary in-container |
| 17.7 | Launch an agent in a tmux window (claude via run-claude, remote control on) | ❌ | 5 | launch; new `agent-*` window; pair from phone |
| 17.8 | List blocked egress URLs | ❌ | 4 | trigger a block; menu → blocked egress lists it |
| 17.9 | Add allowlist hosts (persist + regen file + offer restart) | ❌ | 5 | add a host; confirm allowlist file + that the host is reachable after restart |
| 17.10 | Open root terminal (new `root-bash` window, uid 0) | ❌ | 5 | menu → root terminal; `id` shows root |
| 17.11 | Open dev terminal (new `dev-bash` window, uid 1000) | ❌ | 4 | menu → dev terminal; `id` shows dev |
| 17.12 | Send test notification | ❌ | 5 | menu → test notify; observe popup/phone |
| 17.13 | Recreate / reset from the menu (respawns dev-container window; reset guarded by dirty-git scan + typed 'yes') | ❌ | 4 | run each; confirm the window restarts and the container rebuilds |
| 17.14 | Restart (podman restart in place) / Stop (podman stop) — error cleanly when absent | ❌ | 3 | menu → Restart, Stop; observe container state |
| 17.15 | Destroy — double confirm (yes/no + dirty scan + typed 'yes'), wipes container + volume, offers to delete the local deploy key + `.pub` | ❌ | 4 | menu → Destroy; verify volume gone + key-deletion prompt |

## 18. Notifications (M7 — `notify.rs` core + cli)

| # | Test case | Automated | Manual | How to verify |
|---|-----------|:---------:|:------:|---------------|
| 18.1 | Event whitelist rejects unknown events | ✅ `rejects_unknown_events` | 0 | — |
| 18.2 | Label sanitized to `[A-Za-z0-9._-]`, capped at 40 | ✅ `sanitizes_and_caps_label` | 0 | — |
| 18.3 | Title uses label when present; body per event | ✅ `title_uses_label_when_present` | 0 | — |
| 18.4 | FIFO created, event delivered end-to-end → desktop popup + sound | ❌ | 5 | run a task in-container; watch the host popup |
| 18.5 | ntfy.sh push fires when enabled | ❌ | 5 | enable ntfy; trigger; check the phone app |
| 18.6 | Two-hop forward (remote → laptop over TCP/ssh tunnel) renders locally | ❌ | 5 | set `RC_FORWARD_ADDR` + `notify-listen` on laptop; trigger |
| 18.7 | notify-listen forces local render (no re-forward) | ⚠️ (env logic) | 4 | run `notify-listen`; confirm it renders, doesn't bounce |

## 19. Install / distribution (M8 — `install.rs`)

| # | Test case | Automated | Manual | How to verify |
|---|-----------|:---------:|:------:|---------------|
| 19.1 | `introdus install` copies binary to `~/.local/bin`, chmod +x | ❌ | 3 | `introdus install`; `ls -l ~/.local/bin/introdus` |
| 19.2 | Idempotent when already installed (same-file detection) | ❌ | 2 | run twice; second says "already installed" |
| 19.3 | PATH guidance branch (on-PATH vs not) | ❌ | 2 | run with/without the dir on PATH |

## 20. CLI surface & docs (M9)

| # | Test case | Automated | Manual | How to verify |
|---|-----------|:---------:|:------:|---------------|
| 20.1 | `--help` and each subcommand `help` render | ⚠️ smoke-tested manually | 1 | `introdus help <sub>` for all 11 |
| 20.2 | `--version` matches crate version | ⚠️ | 1 | `introdus --version` |
| 20.3 | README/`sample.env` match actual behaviour | ❌ | 2 | follow the README quickstart verbatim |

## 21. End-to-end integration (M10)  ← the decisive pass

Much of this is now automated by the **full-experience harness** (rootless
podman-in-podman): `test-harness/harness.sh` drives the real `introdus launch`
→ tmux session → dev container → egress firewall → public-repo clone → live
control TUI and asserts on it. Heavy + opt-in (needs a rootless-podman host with
`/dev/fuse` + `/dev/net/tun`), so not part of `cargo test`. See
`test-harness/README.md`.

| # | Test case | Automated | Manual | How to verify |
|---|-----------|:---------:|:------:|---------------|
| 21.1 | Fresh project: `launch` → tmux session (main-control/notify/dev-container) → container up + clone → live menu | ✅ harness `menu` | 2 | `test-harness/harness.sh menu` |
| 21.2 | Egress self-check green; allowlisted reachable, others + direct-IP dropped | ✅ harness `verify` | 1 | `test-harness/harness.sh verify` |
| 21.3 | Menu dispatches into the running container (open a dev terminal → `uid=1000(dev)`) | ✅ harness `menu` | 1 | asserted in driver-menu.sh |
| 21.4 | Persistence across recreate (repo, node_modules, claude auth survive) | ❌ | 4 | recreate; confirm `/home/dev` intact |
| 21.5 | Drive Claude from phone via remote control | ❌ | 5 | pair and issue a prompt from the mobile app |
| 21.6 | `.env` parity: generated vs a `./launch.sh` run behave identically | ❌ | 4 | diff both `.env`s and both containers' `podman inspect` |

---

## Coverage summary

- **Fully automated (rating 0):** the pure logic core — config round-trip,
  `.env` quoting, agent registry invariants, naming/suffix, egress regex &
  ordering, extra-port validation, session-name generation, notification
  trust-boundary (event whitelist + label sanitization), `podman run` flag
  assembly, `Cmd`/podman/tmux arg building, asset embedding/materialization.
- **Interactive TUI (now pty-automated):** the wizard prompts end-to-end incl.
  the generate-new-key and reuse-matching-key branches (16.2–16.4, 16.6) and the
  menu's render/group/quit + the `no such container` regression (17.1, 17.1a/b)
  are driven through a real pty by `rexpect` — no live host needed.
- **Full experience (now harness-automated):** the real `introdus launch` →
  tmux session → nested dev container → egress firewall → clone → live control
  TUI is driven and asserted by the rootless podman-in-podman harness
  (`test-harness/harness.sh`, targets `verify`/`launch`/`menu`) — covering
  21.1–21.3 and the podman-backed egress path. Heavy + opt-in, not in
  `cargo test`.
- **Highest manual reliance (rating 5):** anything that needs a live
  rootless-podman host, tmux, a desktop/phone, or the network — real egress
  enforcement (9.5, 11.1), container boot & privilege drop (10.7), the
  podman-backed menu actions (17.2–17.15), notification delivery (18.4–18.6),
  base-image build (8.2), the **reset/destroy data-loss safety scan** (13.4–13.11
  — unstaged/staged/untracked changes, unpushed commits, and stashes, plus the
  always-required typed confirm), and the end-to-end pass (21.*).

The security-critical *inputs* (allowlist patterns, run flags, trust-boundary
sanitization) are automated at rating 0; the security-critical *enforcement*
(the proxy/nft actually dropping traffic) is inherently rating 5 and must be
observed on a real host — cross-check against a `./launch.sh` container.
