# Agent instructions

- Performance is important, but it's not worth optimizing for at the cost of readability and maintainability.
  - Especially true for borrow checker struggles in Rust. If the code is more readable with some clones, it's definitely worth it.
- Just before marking a task complete, consider if the changes just made need an update to the documentation in `agent_rules/03_source-code-overview.md`
- Always run `./scripts/lint.sh` --full before marking a task complete
  - If any errors are reported, fix them and run the command again until it reports no errors.
- You can run `./scripts/lint.sh --quick` often since it completes fast and gives you quick feedback on linter rules conformance