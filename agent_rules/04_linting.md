# Linting

What each linter checks, where its config lives, how to run it standalone, and
how to install it. Single entry point is [`scripts/lint.sh`](../scripts/lint.sh);
this doc explains what's inside the box. The workspace is **Rust-only** — there
is no Kotlin/Android tooling.

## Modes (each is a strict superset of the previous)

| Mode         | Runs                                                                | Typical use                |
| ------------ | ------------------------------------------------------------------- | -------------------------- |
| `--quick`    | rustfmt + clippy                                                    | manual dev (~10 s warm)    |
| `--full`     | adds tokei budget, cargo-machete, cargo-deny, cargo-audit, jscpd    | CI / on demand             |
| `--security` | adds semgrep (Rust rule pack)                                       | default pre-commit         |

Default is `--quick`.

**No silent skips.** If a tool the mode would run isn't installed, the script
records a failure (accumulating across the whole run so you see every install you
owe in one pass) with the install hint and exits non-zero.

## Pre-commit hook

[`scripts/install-pre-commit.sh`](../scripts/install-pre-commit.sh) installs a
hook that runs **`cargo test --workspace` then `scripts/lint.sh <mode>`**:

```bash
./scripts/install-pre-commit.sh             # default: --security
./scripts/install-pre-commit.sh --full      # everything except semgrep
./scripts/install-pre-commit.sh --quick     # fmt + clippy only
```

Idempotent, but refuses to clobber a hook it didn't write. Bypass a single
commit with `git commit --no-verify`.

## The linters

### rustfmt

What: enforces a single canonical Rust source format.

Config: [`rustfmt.toml`](../rustfmt.toml) at repo root (stable options only;
`max_width = 100`, 4-space, Unix newlines).

```bash
cargo fmt --all -- --check     # what lint.sh runs
cargo fmt --all                # apply formatting
```

Bundled with rustup; nothing to install separately.

### clippy

What: Rust's official linter. Lint level is `-D warnings` (every warning is an
error). We additionally opt into the nursery `cognitive_complexity` lint via
[`Cargo.toml`](../Cargo.toml) `[workspace.lints.clippy]` (it is *not* covered by
`-D warnings`), with the threshold dropped to **15** in
[`clippy.toml`](../clippy.toml). Per-site overrides go in source with
`#[allow(clippy::lint_name)] // why: …`.

```bash
cargo clippy --workspace --all-targets --no-deps -- -D warnings
```

Bundled with rustup.

### tokei (file-size budget)

What: per-Rust-file budget of **≤ 600 lines of code** — a soft signal that a
module wants splitting.

Config: budget is a literal in [`scripts/lint.sh`](../scripts/lint.sh) (the
Python one-liner after the `tokei` call). Tokei itself takes no config.

```bash
tokei -t Rust crates/
```

Install: `cargo install tokei`.

### cargo-machete

What: finds Cargo dependencies declared in a `Cargo.toml` but not actually used.

Config: optional `[package.metadata.cargo-machete] ignored = [...]` in a crate's
`Cargo.toml` for known false positives.

```bash
cargo machete
cargo machete --with-metadata   # check against Cargo.lock instead of the grep heuristic
```

Install: `cargo install cargo-machete`.

### cargo-deny

What: dependency / license / advisory policy. Refuses unknown registries, unknown
git sources, disallowed licenses, and yanked crates.

Config: [`deny.toml`](../deny.toml) at repo root (`licenses`, `bans`, `sources`,
`advisories`). Advisory ignores mirror `.cargo/audit.toml`.

```bash
cargo deny --no-default-features check                # everything (what lint.sh runs)
cargo deny --no-default-features check advisories     # only the RustSec db
cargo deny --no-default-features check licenses|bans|sources
```

Install: `cargo install cargo-deny --locked`.

### cargo-audit

What: scans `Cargo.lock` against the RustSec advisory database.

Config: [`.cargo/audit.toml`](../.cargo/audit.toml) — per-CVE waivers under
`[advisories].ignore`, each justified in a comment above it (kept in sync with
`deny.toml`'s advisory ignores).

```bash
cargo audit
```

Install: `cargo install cargo-audit --locked`.

### jscpd (copy-paste duplication)

What: spots copy-pasted code blocks across the Rust sources.

Config: passed on the command line in [`scripts/lint.sh`](../scripts/lint.sh).
The gate is `--exitCode 1` (fail the moment any clone is detected), **not**
`--threshold N` — a percentage threshold lets real duplication slip past at low
percentages. Drive duplication out via refactoring, never by softening this gate.

Standalone (from the repo root):

```bash
./tools/node_modules/.bin/jscpd \
  --pattern "crates/**/*.rs" \
  --ignore "**/target/**,**/node_modules/**" \
  --min-lines 8 --min-tokens 50 \
  --exitCode 1 --reporters consoleFull .
```

Install (local, via pnpm — keeps the global npm namespace clean):

```bash
cd tools && pnpm install
```

`tools/` holds [`package.json`](../tools/package.json) and `pnpm-lock.yaml`
(committed); `tools/node_modules/` is gitignored.

### semgrep

What: SAST / pattern-based static analysis. We pin to the Rust rule pack.

Config: command-line `--config p/rust`. Per-call suppressions go on the source
line above the match as `// nosemgrep: rule.id` plus a why-comment.

```bash
semgrep --error --config p/rust --exclude target --exclude node_modules .
```

Install: `pipx install semgrep`.

## Suppression patterns by linter

Silence a genuine false positive at the one site, with a comment documenting
*why*:

| Linter        | Suppression                                                                        |
| ------------- | ---------------------------------------------------------------------------------- |
| clippy        | `#[allow(clippy::lint_name)]` on the item; add a `// why:` line.                    |
| rustfmt       | `#[rustfmt::skip]` on an item, or `// rustfmt-skip` on a single line.               |
| cargo-machete | `[package.metadata.cargo-machete] ignored = ["..."]` in the crate's `Cargo.toml`.  |
| cargo-deny    | edit `deny.toml`'s `[licenses]` / `[bans]` / `[advisories]` sections explicitly.   |
| cargo-audit   | `.cargo/audit.toml` with documented `[advisories].ignore` entries.                 |
| semgrep       | `// nosemgrep: rule.id` on the line immediately above the match.                    |

## Adding a new linter

1. Add the tool to one of the modes in `scripts/lint.sh` — guard it with
   `require_cmd` so a missing install is reported as a failure, not skipped.
2. If it has a config file, put it at repo root with the rest
   ([`rustfmt.toml`](../rustfmt.toml), [`clippy.toml`](../clippy.toml),
   [`deny.toml`](../deny.toml)).
3. Document it in this file: what / config / standalone / install.
