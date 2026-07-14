# Agent instructions

## Where these rules live (single source of truth)

`agent_rules/` is the **only** place to edit project rules. Every agent is fed
these same files, but each expects them somewhere different, so we mirror
`agent_rules/` out to each without duplicating content:

- **Claude Code** reads `.claude/rules/*.md` — these are **symlinks** into
  `../../agent_rules/*.md` (one symlink per rule file). Editing a source file is
  picked up automatically; no regeneration needed.
- **Codex, Pi, and opencode** each auto-load a single root **`AGENTS.md`** and
  cannot glob a rules directory. `AGENTS.md` is therefore a **generated file** —
  `agent_rules/*.md` concatenated by
  [`scripts/gen-agent-rules.sh`](../scripts/gen-agent-rules.sh).

**Never edit `.claude/rules/*.md` or `AGENTS.md` directly.** Edit the file under
`agent_rules/`, then run `./scripts/gen-agent-rules.sh` to refresh `AGENTS.md`
(the symlinks need nothing). Adding a brand-new rule file also needs a matching
symlink: `ln -s ../../agent_rules/NN_name.md .claude/rules/NN_name.md`.
`./scripts/lint.sh --quick` fails if `AGENTS.md` has drifted from the sources.

## General

- Performance is important, but it's not worth optimizing for at the cost of readability and maintainability.
  - Especially true for borrow checker struggles in Rust. If the code is more readable with some clones, it's definitely worth it.
- Just before marking a task complete, consider if the changes just made need an update to the documentation in `agent_rules/03_source-code-overview.md`
- Always run `./scripts/lint.sh` --full before marking a task complete
  - If any errors are reported, fix them and run the command again until it reports no errors.
- You can run `./scripts/lint.sh --quick` often since it completes fast and gives you quick feedback on linter rules conformance