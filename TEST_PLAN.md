# introdus — Test Plan

A feature-by-feature test catalogue for the `introdus` Rust control plane.
Consult it when validating a feature. Each case has a **stable ID** (`TAnn`), an
**Automated** marker, and a **manual-reliance rating (0–5)**.

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

## Test IDs and the Automated column

Every row has a stable `TAnn` ID (never renumbered — new cases get the next free
number). The automated test backing a row is **named with that ID in the code**,
so the ID is the link between plan and code:

- Find / run it: `rg ta06` or `cargo test ta06` (a row backed by several test
  functions shares the ID prefix, e.g. `ta25_*`).
- **Automated** column: `✅` = a `taNN_…` test covers it · `⚠️` = only helper
  logic is tested · `❌` = none · `→TAxx` = covered by the test owned by row
  `TAxx` · **harness `<target>`** = asserted by `test-harness/harness.sh
  <target>` (the `driver-*.sh` scripts), not `cargo test`.

Run the fast suite with `cargo test --workspace` and the quality gates with
`scripts/lint.sh --full` (or `--security`, which also runs semgrep). The
pre-commit hook (`scripts/install-pre-commit.sh`) runs **both** — `cargo test`
then the lint suite — on every commit.

The interactive TUI is covered by **pty integration tests** under
`crates/introdus-cli/tests/` (`wizard_pty.rs`, `menu_pty.rs`), which spawn the
real binary through a pseudo-terminal via `rexpect`. The whole UI is `ratatui`
now (no `inquire`): the wizard is a sequence of inline modal prompts driven with
explicit keystrokes (a bare test pty can't answer the DSR cursor-position query,
so the modals fall back to a fixed viewport there); the persistent control menu
is a full-screen app, so `menu_pty.rs` only smoke-tests its start/quit and the
no-leak guarantee — its on-screen layout is exercised by the tmux harness
(`test-harness/driver-menu.sh`). These need no podman/tmux (the wizard is reached
through the standalone `introdus init`), so they run anywhere `cargo test` does.

The **full experience** — real `introdus launch` → tmux session → nested rootless
podman dev container → egress firewall → public-repo clone → live control TUI —
is driven and asserted by the **rootless podman-in-podman harness**
(`test-harness/harness.sh`, targets `verify` / `menu` / `egress` / `lifecycle` /
`all`). It's heavy + opt-in (needs a rootless-podman host with `/dev/fuse` +
`/dev/net/tun`), so NOT part of `cargo test`. See `test-harness/README.md`.

---

## 1. Build & quality gates (M0)

| ID | Test case | Automated | Manual | How to verify |
|----|-----------|:---------:|:------:|---------------|
| TA01 | Workspace compiles (debug + release) | ⚠️ | 1 | `cargo build --workspace && cargo build --release` |
| TA02 | `scripts/lint.sh --full` passes (fmt, clippy, deny, audit, machete, tokei, jscpd) | ✅ | 0 | `./scripts/lint.sh --full` (the gate itself) |
| TA03 | `scripts/lint.sh --security` passes (adds semgrep) | ✅ | 1 | needs a working `semgrep` (`pipx reinstall semgrep` if broken) |

## 2. Config & `.env` round-trip (M1 — `config.rs`, `env_file.rs`)

| ID | Test case | Automated | Manual | How to verify |
|----|-----------|:---------:|:------:|---------------|
| TA06 | `render` → `load` is lossless for a fully-populated config | ✅ | 0 | — |
| TA07 | Defaults applied for a minimal `.env` (agents, whitelist, mem, pids, timeout, canary) | ✅ | 0 | — |
| TA08 | Missing required field errors (`REPO_URL`, etc.) | ✅ | 0 | — |
| TA09 | Multi-line `WHITELIST_HOSTS` / `ON_LAUNCH_SCRIPT` parse (bash-quoted) | ✅ (→TA06 + `ta09_*`) | 1 | hand-write a multi-line `.env`, `introdus verify` reads it |
| TA10 | Value quoting escapes `"`, `\`, `$`, backtick correctly | ✅ | 0 | — |
| TA11 | An existing hand-written `.env` (from the bash flow) loads unchanged in meaning | ⚠️ | 2 | load a real project `.env`, diff `render` output for surprises |
| TA12 | Saving normalizes/rewrites the file (comments regenerated) | ⚠️ | 2 | run a menu action that saves; inspect the `.env` |

## 3. Agent registry (M1 — `agents.rs`)

| ID | Test case | Automated | Manual | How to verify |
|----|-----------|:---------:|:------:|---------------|
| TA14 | Script-method agents use URL specs | ✅ | 0 | — |
| TA15 | Registry stays in sync with `container/agents.sh` | ❌ (hand-kept) | 3 | diff the two by eye when either changes |
| TA16 | Each agent's egress hosts are actually sufficient to auth | ❌ | 5 | install the agent, sign in, watch `egress-log` for blocks |

## 4. Naming & paths (M1 — `names.rs`, `paths.rs`)

| ID | Test case | Automated | Manual | How to verify |
|----|-----------|:---------:|:------:|---------------|
| TA17 | Container/volume/image names carry the suffix | ✅ | 0 | — |
| TA18 | Image slug sanitizes uppercase/space/punctuation | ✅ | 0 | — |
| TA19 | Fallback suffix deterministic, 4 hex, differs per host | ✅ | 0 | — |
| TA20 | State/allowlist path under `$XDG_STATE_HOME/introdus` | ✅ | 0 | — |

## 5. Embedded assets (M2 — `assets.rs`)

| ID | Test case | Automated | Manual | How to verify |
|----|-----------|:---------:|:------:|---------------|
| TA21 | All 11 assets embedded, non-empty; entrypoint contains `nft` | ✅ | 0 | — |
| TA22 | Materialize writes the tree with correct exec/non-exec modes | ✅ | 0 | — |
| TA23 | Materialized build context actually `podman build`s | ✅ harness `verify` | 1 | the harness builds the base image nested from materialized assets |
| TA24 | Embedded bash byte-identical to `container/` sources | ⚠️ (include_str! guarantees) | 1 | `git diff` shows no drift; rebuild after edits |

## 6. Process / podman / tmux wrappers (M2)

| ID | Test case | Automated | Manual | How to verify |
|----|-----------|:---------:|:------:|---------------|
| TA25 | `Cmd` arg/label building, exit-code mapping, stdout capture, ok-probe | ✅ (`ta25_*`) | 0 | — |
| TA26 | `podman exec` / `exec -it` flag building (`--user`) | ✅ (`ta26_*`) | 0 | — |
| TA27 | `tmux attach` label | ✅ | 0 | — |
| TA28 | The wrappers drive real podman/tmux correctly | ⚠️ harness (exercised throughout) | 2 | every harness launch/menu/exec action drives them for real |

## 7. Preflight checks (M3 — `preflight.rs`)

| ID | Test case | Automated | Manual | How to verify |
|----|-----------|:---------:|:------:|---------------|
| TA29 | Errors on non-Linux / root / missing podman / missing pasta / non-rootless | ❌ | 3 | temporarily rename `pasta`; run `introdus up`; expect a clear error |
| TA30 | `check_session` additionally requires tmux | ❌ | 3 | rename `tmux`; run `introdus`; expect the tmux hint |
| TA31 | Passes cleanly on a correct host | ⚠️ | 2 | `introdus up` gets past preflight into `.env`/wizard logic |

## 8. Base image build / tag / prune (M3 — `image.rs`)

| ID | Test case | Automated | Manual | How to verify |
|----|-----------|:---------:|:------:|---------------|
| TA32 | Stale project-tag matcher (`introdus-<slug>-XXXX:latest`) | ✅ | 0 | — |
| TA33 | Builds the base image when missing | ✅ harness `verify` | 2 | the harness builds it nested on a clean volume; manual for cache nuances |
| TA34 | Cached rebuild when the binary is newer than the image | ❌ | 4 | rebuild introdus, relaunch, watch for the "cached rebuild" line |
| TA35 | `rebuild-base` forces `--no-cache` | ❌ | 4 | `introdus rebuild-base`; confirm layers rebuild |
| TA36 | Per-project tag applied; stale suffixed tags pruned | ❌ | 3 | change `IMAGE_SUFFIX`, relaunch, `podman image ls` |

## 9. Egress allowlist generation (M3 — `egress.rs`)  ← security-critical

| ID | Test case | Automated | Manual | How to verify |
|----|-----------|:---------:|:------:|---------------|
| TA37 | Git-host extraction across `git@`/`ssh://`/`https://`/bare forms | ✅ | 0 | — |
| TA38 | Allowlist regex escaping matches the shell's `sed` | ✅ | 0 | — |
| TA39 | Ordered whitelist = git host + WHITELIST + tunnel host | ✅ | 1 | assert the generated allowlist matches the expected ordered patterns |
| TA40 | Rendered allowlist file = one pattern per line | ✅ | 0 | — |
| TA41 | **Proxy actually enforces the allowlist in the container** | ✅ harness `egress` | 1 | driver-egress.sh: allowed via proxy ✓, non-allowlisted ✗, direct dial dropped, `egress-log` shows it |

## 10. Container create — `podman run` flag set (M3 — `run.rs`)  ← security-critical

| ID | Test case | Automated | Manual | How to verify |
|----|-----------|:---------:|:------:|---------------|
| TA42 | Hardening flags present (`--cap-drop=ALL`, `no-new-privileges`, `pasta`, image→entrypoint) | ✅ | 1 | `podman inspect` the running container |
| TA43 | `--disable-network-block` drops `NET_ADMIN` and sets the env | ✅ | 2 | launch with the flag; confirm unfiltered egress |
| TA44 | Webapp + extra ports published to 127.0.0.1 | ✅ | 1 | `podman port`, hit the port from host |
| TA45 | Extra-port parse/validate (bad, out-of-range, collision) | ✅ (`ta45_*`) | 0 | — |
| TA46 | All five bind-mounts + `/run/notify` + shared-data present | ⚠️ (built in TA42) | 2 | `podman inspect` mounts on a live container |
| TA47 | Deploy-key / shared-data existence validation | ⚠️ (`validate_inputs`, no test) | 2 | point `.env` at a missing key; expect a clear error |
| TA48 | Container actually boots, entrypoint drops to `dev` | ✅ harness `menu` | 1 | driver-menu.sh: dev terminal shows uid=1000(dev); egress self-check passes |

## 11. Egress self-check — `introdus verify` (M3)

| ID | Test case | Automated | Manual | How to verify |
|----|-----------|:---------:|:------:|---------------|
| TA49 | Canary blocked, proxy reaches allowlisted host, direct-IP blocked | ✅ harness `verify` | 1 | `test-harness/harness.sh verify` → "verify passed" nested |
| TA50 | Verify aborts the launch on any failure | ❌ | 4 | remove the git host from WHITELIST; expect failure |

## 12. In-container update — `introdus update` (M3)

| ID | Test case | Automated | Manual | How to verify |
|----|-----------|:---------:|:------:|---------------|
| TA51 | Errors if the container isn't running | ⚠️ | 2 | run against a stopped container |
| TA52 | apt/mise/claude/agents/lazyvim refresh runs through the proxy | ❌ | 5 | `introdus update`; watch it complete without egress blocks |

## 13. Lifecycle — recreate / reset / pull (M3, M6 — `lifecycle.rs`)

| ID | Test case | Automated | Manual | How to verify |
|----|-----------|:---------:|:------:|---------------|
| TA53 | Legacy pre-suffix container removed | ❌ | 3 | create a legacy-named container; relaunch |
| TA54 | Recreate drops container, keeps volume | ✅ harness `lifecycle` | 1 | driver-lifecycle.sh: marker survives recreate |
| TA55 | Reset/destroy wipes the volume | ✅ harness `lifecycle` | 1 | driver-lifecycle.sh: destroy removes the volume |
| TA56 | Reset scan detects **unstaged working-tree changes** | ✅ harness `lifecycle` | 1 | driver-lifecycle.sh plants a modified tracked file; scan reports "working tree" |
| TA57 | Reset scan detects **staged-but-uncommitted changes** | ❌ | 4 | `git add` a change; reset; scan lists it (`git status --porcelain` shows both) |
| TA58 | Reset scan detects **untracked files** | ✅ harness `lifecycle` (via "working tree") | 2 | driver-lifecycle.sh plants an untracked file; `??` appears under working tree |
| TA59 | Reset scan detects **unpushed commits** (not on any remote) | ✅ harness `lifecycle` | 1 | driver-lifecycle.sh commits locally; scan reports "unpushed commits: N" |
| TA60 | Reset scan detects **stashes** | ❌ | 4 | `git stash`; reset; scan lists the stash |
| TA61 | Scan walks **every repo** under `/home/dev/work` (multi-repo) | ❌ | 4 | dirty two repos; reset; both appear in the report |
| TA62 | Typed `yes` confirmation required (destroy/reset) | ⚠️ harness `lifecycle` (typed `yes` exercised) | 3 | harness sends the typed `yes`; manual for the "clean volume still demands yes" + non-`yes`-aborts branch |
| TA63 | Scan is read-only and non-fatal (best-effort; failure never blocks the confirm) | ✅ harness `lifecycle` | 2 | driver-lifecycle.sh: scan runs on a `:ro` mount and the flow reaches the `yes` prompt |
| TA64 | `--pull` sentinel triggers a ff-only pull on next start | ❌ | 4 | `introdus up --pull`; confirm the repo fast-forwards |

## 14. tmux session model — `introdus launch` (M4 — `session.rs`)

| ID | Test case | Automated | Manual | How to verify |
|----|-----------|:---------:|:------:|---------------|
| TA65 | `window_cmd` builds `exec '<bin>' <sub>` | ✅ | 0 | — |
| TA66 | Session name minted + persisted to `.env` on first launch | ⚠️ (generator tested, →TA70) | 3 | first `introdus`; grep `SESSION_NAME` in `.env` |
| TA67 | Session created with main-control + notify + dev-container windows | ✅ harness `menu` | 1 | driver-menu.sh asserts all three windows exist |
| TA68 | Re-launch re-attaches instead of spawning a duplicate | ❌ | 3 | run `introdus` twice |
| TA69 | Wizard runs when `.env` is absent, then launches | ❌ | 4 | `introdus` in an empty dir |

## 15. Session naming (M4 — `session.rs` core)

| ID | Test case | Automated | Manual | How to verify |
|----|-----------|:---------:|:------:|---------------|
| TA70 | Deterministic per project, `introdus-adj-adj-noun` shape | ✅ | 0 | — |
| TA71 | Two adjectives differ; distinct across projects | ✅ (`ta71_*`) | 0 | — |

## 16. Setup wizard (M5 — `wizard.rs`)

| ID | Test case | Automated | Manual | How to verify |
|----|-----------|:---------:|:------:|---------------|
| TA72 | Selected agents' egress hosts appended to whitelist | ✅ | 0 | — |
| TA73 | Prompts: name/repo/port/agents/tunnel/ntfy flow end-to-end | ✅ (→TA74, →TA75) | 1 | `cargo test --test wizard_pty`; or walk it live |
| TA74 | Deploy key — "generate new?" asked first; **yes** → prompts *where to create* (default `~/.ssh/introdus-deploy-keys/<slug>-deploy-key`, dir chmod 700), writes the keypair, prints the `.pub`, refuses to overwrite | ✅ (`ta74_*`) | 2 | pty test covers the happy path; manual for chmod 700 + overwrite-refusal |
| TA75 | Deploy key — **no** → offers a project-matching key to reuse (yes/no; picker when several), else prompts for an existing path; registration shown either way | ✅ | 2 | pty test covers reuse; manual for the bad-path re-ask |
| TA76 | Wizard writes a valid, loadable `.env` | ✅ (→TA74, →TA75; + round-trip TA06) | 1 | pty tests read back the written `.env` |
| TA77 | Cancel (Esc/Ctrl-C) aborts cleanly | ❌ | 3 | Esc mid-wizard |
| TA78 | `introdus init` runs the wizard standalone (no podman); confirms before overwriting an existing `.env` | ✅ (→TA74, →TA75 invoke `init`) | 2 | `cargo test --test wizard_pty`; manual for the overwrite confirm |

## 17. Control TUI + utilities (M6 — `menu.rs`, `menu_actions.rs`)

| ID | Test case | Automated | Manual | How to verify |
|----|-----------|:---------:|:------:|---------------|
| TA79 | Menu loop renders status header, dispatches, survives action errors | ⚠️ (→TA80) | 3 | `cargo test --test menu_pty`; live for dispatch/error paths |
| TA80 | Absent container reads as "not created" — no leaked `Error: no such container` | ✅ | 0 | pty regression test (`ta80_*`) |
| TA81 | Grouped sections render (inert headers) and the whole menu shows at once | ✅ (→TA80) | 1 | pty test asserts a section header; eyeball the full layout |
| TA82 | Show tunnel URL | ❌ | 5 | with `EXPOSE_WEBAPP`, menu → tunnel URL prints the trycloudflare URL |
| TA83 | Toggle expose-webapp (persist + offer recreate) | ❌ | 4 | toggle; grep `.env`; recreate; confirm tunnel starts |
| TA84 | Enable ntfy (topic prompt + persist) | ❌ | 4 | enable; grep `.env`; recreate; check phone |
| TA85 | Copy a host file/folder into the container | ✅ harness `menu` | 1 | driver-menu.sh asserts the file in /home/dev/uploads |
| TA86 | Install an agent at runtime (persist + whitelist + run install-agents) | ✅ harness `install` →TA115 | 1 | driver-install.sh installs codex; `.env`/whitelist updated + package present |
| TA87 | Launch an agent in a tmux window (claude via run-claude, remote control on) | ❌ | 5 | launch; new `agent-*` window; pair from phone |
| TA88 | List blocked egress URLs | ✅ harness `egress` | 1 | driver-egress.sh triggers a block, menu lists it |
| TA89 | Add allowlist hosts (persist + regen file + offer restart) | ✅ harness `menu` (persist + offer) | 2 | driver-menu.sh asserts .env; manual for post-restart reachability |
| TA90 | Open root terminal (new `root-bash` window, uid 0) | ✅ harness `menu` | 1 | driver-menu.sh asserts uid=0(root) |
| TA91 | Open dev terminal (new `dev-bash` window, uid 1000) | ✅ harness `menu` | 1 | driver-menu.sh asserts uid=1000(dev) |
| TA92 | Send test notification | ❌ | 5 | menu → test notify; observe popup/phone |
| TA93 | Recreate from the menu (respawns dev-container window, keeps volume) | ✅ harness `lifecycle` | 1 | driver-lifecycle.sh: marker survives recreate |
| TA94 | Restart (podman restart in place) / Stop (podman stop) — error cleanly when absent | ✅ harness `menu` | 1 | driver-menu.sh asserts stopped→running transitions |
| TA95 | Destroy — double confirm (yes/no + dirty scan + typed 'yes'), wipes container + volume, deletes the local deploy key + `.pub` | ✅ harness `lifecycle` | 1 | driver-lifecycle.sh asserts full teardown + key deleted |

## 18. Notifications (M7 — `notify.rs` core + cli)

| ID | Test case | Automated | Manual | How to verify |
|----|-----------|:---------:|:------:|---------------|
| TA96 | Event whitelist rejects unknown events | ✅ | 0 | — |
| TA97 | Label sanitized to `[A-Za-z0-9._-]`, capped at 40 | ✅ | 0 | — |
| TA98 | Title uses label when present; body per event | ✅ | 0 | — |
| TA99 | FIFO created, event delivered end-to-end → desktop popup + sound | ❌ | 5 | run a task in-container; watch the host popup |
| TA100 | ntfy.sh push fires when enabled | ❌ | 5 | enable ntfy; trigger; check the phone app |
| TA101 | Two-hop forward (remote → laptop over TCP/ssh tunnel) renders locally | ❌ | 5 | set `RC_FORWARD_ADDR` + `notify-listen` on laptop; trigger |
| TA102 | notify-listen forces local render (no re-forward) | ⚠️ (env logic) | 4 | run `notify-listen`; confirm it renders, doesn't bounce |

## 19. Install / distribution (M8 — `install.rs`)

| ID | Test case | Automated | Manual | How to verify |
|----|-----------|:---------:|:------:|---------------|
| TA103 | `introdus install` copies binary to `~/.local/bin`, chmod +x | ❌ | 3 | `introdus install`; `ls -l ~/.local/bin/introdus` |
| TA104 | Idempotent when already installed (same-file detection) | ❌ | 2 | run twice; second says "already installed" |
| TA105 | PATH guidance branch (on-PATH vs not) | ❌ | 2 | run with/without the dir on PATH |

## 20. CLI surface & docs (M9)

| ID | Test case | Automated | Manual | How to verify |
|----|-----------|:---------:|:------:|---------------|
| TA106 | `--help` and each subcommand `help` render | ⚠️ smoke-tested manually | 1 | `introdus help <sub>` for all subcommands |
| TA107 | `--version` matches crate version | ⚠️ | 1 | `introdus --version` |
| TA108 | README/`sample.env` match actual behaviour | ❌ | 2 | follow the README quickstart verbatim |

## 21. End-to-end integration (M10)  ← the decisive pass

Automated by the **full-experience harness** (rootless podman-in-podman):
`test-harness/harness.sh` drives the real `introdus launch` → tmux session → dev
container → egress firewall → public-repo clone → live control TUI and asserts on
it. Heavy + opt-in (needs a rootless-podman host with `/dev/fuse` +
`/dev/net/tun`), so not part of `cargo test`. See `test-harness/README.md`.

| ID | Test case | Automated | Manual | How to verify |
|----|-----------|:---------:|:------:|---------------|
| TA109 | Fresh project: `launch` → tmux session (main-control/notify/dev-container) → container up + clone → live menu | ✅ harness `menu` | 2 | `test-harness/harness.sh menu` |
| TA110 | Egress self-check green; allowlisted reachable, others + direct-IP dropped | ✅ harness `verify` | 1 | `test-harness/harness.sh verify` |
| TA111 | Menu dispatches into the running container (open a dev terminal → `uid=1000(dev)`) | ✅ harness `menu` | 1 | asserted in driver-menu.sh |
| TA112 | Persistence across recreate (`/home/dev` volume survives) | ✅ harness `lifecycle` | 2 | driver-lifecycle.sh: marker survives; manual for node_modules/claude-auth specifics |
| TA113 | Drive Claude from phone via remote control | ❌ | 5 | pair and issue a prompt from the mobile app |
| TA114 | `.env` parity: a wizard-written vs a hand-written `.env` behave identically (the legacy `./launch.sh` baseline this once diffed against has been removed) | ❌ | 4 | diff both `.env`s and both containers' `podman inspect` |
| TA115 | Runtime agent install streams live progress (spinner) and actually installs — codex (npm) + antigravity (vendor script, whose download host must be allowlisted) | ✅ harness `install` | 1 | `test-harness/harness.sh install` |
| TA116 | A long action disables the menu: keys mashed during an install don't cascade into other actions (a stray Stop is ignored) | ✅ harness `install` | 1 | driver-install.sh: stray Stop during install → container still running |
| TA117 | claude is opt-out: with `INSTALL_AGENTS=""` (nothing selected) claude is genuinely **absent** — nothing prebakes or force-installs it | ✅ harness `agents` | 1 | driver-agents.sh: `command -v claude` fails after launch |
| TA118 | claude is opt-in: installable on demand through the menu via `pnpm --allow-build` (its native binary ships as an npm optionalDependency — no extra egress host) | ✅ harness `agents` | 1 | driver-agents.sh: menu-install claude, then `command -v claude` succeeds |
| TA119 | Launching an agent offers its skip-permissions/auto flag and passes it: accepting claude's prompt launches it with `--dangerously-skip-permissions` | ✅ harness `agent-launch` | 1 | driver-agent-launch.sh: pick claude → confirm → `pgrep -f 'run-claude --dangerously-skip-permissions'` |
| TA120 | Confirm prompts render the choice as highlighted Yes/No options (visible, not clipped) | ✅ harness `agent-launch` | 1 | driver-agent-launch.sh: the skip-permissions confirm shows Yes + No |
| TA121 | Status shows "starting container…" while a launch is underway (per-container marker), not "not created"/"stopped" | ✅ harness `menu` | 1 | driver-menu.sh: drop a fresh launch marker on the stopped container → status flips, reverts when cleared |
| TA122 | notify-host runs detached (no `notify` tmux window) and its per-session log is viewable via the "Show the notification log" menu option | ✅ harness `menu` | 1 | driver-menu.sh: no notify window, `pgrep -f notify-host` up, menu shows the log ("reading FIFO") |
| TA77 | Wizard agents are opt-in: nothing pre-checked (Claude shows `[ ]`), confirming with none ticked writes `INSTALL_AGENTS=""` | ✅ pty `wizard_pty` | 1 | ta77_wizard_agents_are_opt_in_nothing_preselected |
| TA123 | Launching a selected-but-uninstalled agent is caught before launch: the menu reports the missing binary and offers to install it instead of spawning a window that exits 127. Both branches: declining launches nothing (no dead window); accepting installs the agent (correct pnpm global-bin-dir PATH) and launches it | ✅ harness `agent-missing` | 1 | driver-agent-missing.sh: select pi in .env without installing → Launch → "isn't installed" + offer → (1) decline → no `agent-pi` window; (2) accept → pi resolves in-container + `agent-pi` window launches |
| TA124 | "Quit introdus (stop the container)" stops the container and tears down the whole tmux session (every window closed) | ✅ harness `quit-stop` | 1 | driver-quit-stop.sh: pick it → confirm → container not Running + `tmux has-session` fails |
| TA126 | Wizard `apply_paseo` records the opt-in and, when enabled, adds the relay host `paseo.sh` to the allowlist (idempotently); disabled leaves both untouched | ✅ unit `introdus-cli` | 1 | ta126_apply_paseo_sets_flag_and_host |
| TA127 | Wizard paseo opt-in: answering "yes" to the paseo prompt writes `INSTALL_PASEO="true"` and allowlists `paseo.sh`; the round-trip config preserves `install_paseo` | ✅ pty `wizard_pty` + unit `config` | 1 | ta127_wizard_paseo_opt_in_records_flag_and_relay_host; ta06 round-trip covers `install_paseo` |
| TA128 | Paseo end to end via the TUI: "Install paseo" installs `@getpaseo/cli` (INSTALL_PASEO + paseo.sh persisted to `.env`). Paseo does NOT wrap agent launches (headless `paseo run` isn't the intended path) — with paseo installed, "Launch an installed agent" still launches claude **directly** into an `agent-claude` window (no "via paseo" offer; the picker goes straight to the yolo-flag prompt). "Show paseo pairing QR code" spawns a `paseo-qr` window that brings the daemon up. Both the agent and the QR run as a single quoted `podman exec` **inside the container** (asserted via a live `podman exec $cname …` carrying the command's tail), not leaked to the host shell. (A physically paired phone is out of scope — no device; daemon↔relay connectivity is asserted separately in TA130.) | ✅ harness `paseo` | 1 | driver-paseo.sh: opt-in via `.env` → paseo auto-installed on launch → Launch → no "via paseo" offer → `agent-claude` window + in-container `run-claude` assert → QR → `paseo-qr` window + in-container assert |
| TA129 | Long lifecycle ops (restart/stop/recreate/reset/destroy/allowlist-restart/copy) stream as spinner-backed tasks instead of freezing the panel: the state line + footer surface the in-progress label (e.g. "stopping the container", "tearing down the container") while the menu is paused | ✅ harness `menu` + `lifecycle` | 1 | driver-menu.sh: Stop → "stopping the container…" label during the SIGKILL grace; driver-lifecycle.sh: Destroy → "tearing down the container…" label during teardown |
| TA130 | Paseo relay reachability: paseo's daemon dials `relay.paseo.sh` over a WebSocket (`ws`) that ignores `HTTPS_PROXY`, so the proxy hostname allowlist can't carry it — like cloudflared it needs an nft IP bypass. When `INSTALL_PASEO`, launch resolves `relay.paseo.sh` (`agents::paseo::RELAY_HOST`) to `PASEO_RELAY_IPS`, passed into the container and allowed directly on `tcp dport 443` by the nft filter. Enabling paseo from the menu therefore offers a **recreate** (env is frozen at create), not the allowlist-only restart. `INSTALL_PASEO` is also passed into the container so setup's install-agents installs paseo on an opted-in launch/recreate. Without the bypass the daemon's relay handshake times out and phone pairing fails. | ✅ unit `introdus-core` + harness `paseo` | 1 | ta130_paseo_relay_host_is_under_the_proxy_allowlist_host; driver-paseo.sh: opt-in via `.env` → assert `PASEO_RELAY_IPS` env + per-IP nft `:443 accept` + daemon `relay_control_connected` |
| TA131 | The dev-machine `notify-listen` config path is under the config dir (`$XDG_CONFIG_HOME/introdus/notify-listen.env`), distinct from the state dir | ✅ unit `introdus-core` | 0 | ta131_notify_listen_config_under_config_dir |
| TA132 | Per-project config path resolution prefers the canonical `.introdus/config.env`, falls back to a legacy top-level `./.env`, and reports a legacy layout as needing migration | ✅ unit `introdus-cli` | 0 | ta132_env_path_prefers_canonical_then_legacy |
| TA133 | `notify-listen` port parsing accepts a bare `PORT` and `host:PORT`, taking the trailing field; rejects non-numeric | ✅ unit `introdus-cli` | 0 | ta133_parse_port_accepts_bare_and_hostport |
| TA134 | `notify-listen` reverse-tunnel argv: autossh gets `-M 0` + keepalive `-o` opts + the `-R PORT:127.0.0.1:PORT` forward + alias; plain-ssh fallback drops `-M` but keeps the keepalives and forward | ✅ unit `introdus-cli` | 0 | ta134_tunnel_argv_autossh_vs_ssh |
| TA135 | The generated `systemd --user` unit is `WantedBy=default.target`, `Restart=on-failure`, has the correct `ExecStart`, and never enables linger (notifications need the graphical session) | ✅ unit `introdus-cli` | 0 | ta135_render_unit_is_no_linger_default_target |
| TA136 | `introdus init` on a legacy `./.env` project offers to migrate it into `.introdus/config.env`, moves it on accept, then treats the project as configured (offers reconfigure, not first-run wizard) | ✅ pty `wizard_pty` | 1 | ta136_init_migrates_legacy_env_into_introdus_dir |
| TA137 | Bare `introdus notify-listen` (no flags / env / saved config) runs a wizard collecting SSH alias + port + systemd choice; `--dry-run` then prints the resolved listener + tunnel plan without side effects | ✅ pty `notify_listen_pty` | 1 | ta137_notify_listen_wizard_then_dry_run_plan |
| TA138 | The setup wizard asks whether to forward notifications to a separate dev machine over an SSH reverse tunnel; opting in (with a port) writes `RC_FORWARD_ADDR=127.0.0.1:<port>` so a headless remote host forwards to the laptop from the first launch | ✅ pty `wizard_pty` | 1 | ta138_wizard_forward_opt_in_sets_rc_forward_addr |

---

## Coverage summary

- **Fully automated (rating 0):** the pure logic core — config round-trip,
  `.env` quoting, agent registry invariants, naming/suffix, egress regex &
  ordering, extra-port validation, session-name generation, notification
  trust-boundary (event whitelist + label sanitization), `podman run` flag
  assembly, `Cmd`/podman/tmux arg building, asset embedding/materialization.
- **Interactive TUI (pty-automated):** the wizard prompts end-to-end incl. the
  generate-new-key and reuse-matching-key branches (TA73–TA76, TA78) and the
  menu's render/group/quit + the `no such container` regression (TA79–TA81) are
  driven through a real pty by `rexpect` — no live host needed.
- **Full experience (harness-automated):** the real `introdus launch` → tmux
  session → nested dev container → egress firewall → clone → live control TUI is
  driven and asserted by the rootless podman-in-podman harness
  (`test-harness/harness.sh`, targets `verify` / `menu` / `egress` /
  `lifecycle` / `install` / `agents` / `agent-launch`): base build + egress self-check (TA23, TA33, TA49),
  **workload egress enforcement (TA41)**, container boot + privilege drop
  (TA48), session + menu utilities (TA67, TA85, TA88–TA91, TA93–TA95), the
  **reset/destroy data-loss safety scan** (TA54–TA56, TA58, TA59, TA63 —
  planting uncommitted + unpushed state and asserting the scan reports it), the
  **runtime agent install** with live progress + menu-disabled-while-running
  (TA86, TA115, TA116), **claude as an opt-out-able agent** (TA117, TA118 —
  absent when unselected, installable on demand), and the end-to-end pass
  (TA109–TA112). Heavy + opt-in,
  not in `cargo test`.
- **Highest manual reliance (rating 5):** what still needs external services or
  eyes no harness provides — notification delivery to a desktop/phone
  (TA99–TA101), the Cloudflare tunnel + tunnel-URL (TA82), enabling ntfy
  (TA83–TA84), runtime agent install/launch + agent auth egress (TA16, TA86,
  TA87), in-container `update` (TA52), and driving Claude from a phone (TA113).
  Residual scan branches (staged-only TA57, stashes TA60, multi-repo TA61) remain
  manual.

The security-critical *inputs* (allowlist patterns, run flags, trust-boundary
sanitization) are automated at rating 0; the security-critical *enforcement*
(the proxy/nft actually dropping traffic) is now automated by the harness (TA41,
TA49) nested — still worth an occasional cross-check against a real
`introdus` container on a live host.
