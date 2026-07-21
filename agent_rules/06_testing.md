# Testing

Three tiers, fastest first. **Every feature gets coverage at the tier that can
actually prove it** — pure logic as a unit test, interactive UI as a pty test,
real container/tmux/firewall behaviour as a nested-harness driver. Don't ship a
feature with only unit tests if its real behaviour lives in the container.

## 1. Unit tests — `cargo test --workspace`

Fast, hermetic, in-module `#[cfg(test)]` blocks across nearly every file in both
crates (pure logic: config round-trips, egress regex, naming, port parsing,
notify sanitization, process wrapper, agent registry, etc.). This is the
`cargo test` suite the pre-commit hook runs on every commit.

```bash
cargo test --workspace          # everything below the pty/harness tiers
cargo test ta06                 # run the case(s) whose fn name carries an ID
```

Test cases are catalogued in [TEST_PLAN.md](../TEST_PLAN.md) with a **stable
`TAnn` ID**. The backing test function is **named with that ID** (`ta06_…`,
`ta25_*`), so the ID is the link between plan and code — `rg ta06` / `cargo test
ta06` finds it. New cases get the next free number; IDs are never renumbered.

## 2. pty integration tests — `crates/introdus-cli/tests/`

The interactive ratatui UI (wizard + control panel) is driven through a **real
pseudo-terminal** with the `rexpect` dev-dependency, feeding explicit keystrokes
(`\r` is Enter in raw mode) and synchronizing on rendered prompt text. Still run
by plain `cargo test` — no podman needed.

- `wizard_pty.rs` — drives `introdus init`'s inline modal sequence (confirm /
  text / checklist) end to end.
- `menu_pty.rs` — starts the two-pane control panel against a project whose
  container was never created (regression guard: no `no such container` leak),
  asserts Esc quits cleanly.
- `common/mod.rs` — shared fixture (temp project dir, `.env`, deploy-key gen).

The full-screen panel's on-screen *layout* is a cursor-addressed frame not
scraped byte-for-byte here; that's the harness's job (below).

## 3. Full-experience E2E — `test-harness/`

The heavy, **opt-in** tier — not part of `cargo test`. Drives the *real*
experience (`introdus launch` → tmux → rootless podman dev container → egress
firewall → clone → live control TUI) inside a **rootless podman-in-podman**
container and **asserts** on it (any bad assertion → `exit 1`). Requires a
rootless-podman host with `/dev/fuse` and `/dev/net/tun`.

```bash
test-harness/harness.sh            # all (default): the full end-to-end sweep
test-harness/harness.sh verify     # egress firewall self-check only (fast-ish)
test-harness/harness.sh launch     # container up + clone through the proxy
test-harness/harness.sh reattach   # repeat launch from a dir reattaches to one session
test-harness/harness.sh menu       # drive the live control TUI over tmux
test-harness/harness.sh egress     # workload default-deny enforcement
test-harness/harness.sh lifecycle  # recreate persistence + destroy teardown
test-harness/harness.sh install    # binary onto PATH
test-harness/harness.sh agents     # claude opt-out absent + opt-in menu install
test-harness/harness.sh agent-launch / agent-missing / quit-stop / detach / paseo / paseo-direct
test-harness/harness.sh send-files # send a host file into a container via the dual-pane TUI
test-harness/harness.sh cli        # headless subcommands drive a real container
```

Each target is a scripted `driver-*.sh` that drives the real UI over tmux and
asserts. First run builds the nested Ubuntu base image (a few minutes), cached
in the `introdus-harness-storage` volume; force a clean rebuild with
`podman volume rm introdus-harness-storage`. See
[test-harness/README.md](../test-harness/README.md) for how the nesting,
`--privileged` outer flags, and HTTPS clone-mocking work.

In [TEST_PLAN.md](../TEST_PLAN.md), rows proven here are marked **harness
`<target>`** in the Automated column (vs `✅` for `cargo test`, `⚠️` helper-only,
`❌` none).

## Pre-commit gate

`scripts/install-pre-commit.sh` installs a hook that runs `cargo test
--workspace` **then** `scripts/lint.sh` (default `--security`). It refuses to
clobber a hook it didn't write; bypass a single commit with `--no-verify`. See
[04_linting.md](04_linting.md).
