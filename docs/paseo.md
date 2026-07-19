# Paseo orchestrator

> Part of [introdus](../README.md#features). Drive your installed agents from a phone, desktop, web, or CLI client.

[paseo](https://paseo.sh) is an agent **orchestrator** (not a coding agent
itself). It runs your installed [agents](coding-agents.md) under a local daemon
and lets you drive them from a phone/desktop/web/CLI client through the paseo
relay.

## Prerequisites

- One or more [coding agents](coding-agents.md) installed — paseo natively
  supports claude, codex, opencode, and pi.
- The paseo relay host `paseo.sh` is added to the
  [allowlist](egress-filtering.md) automatically when paseo is enabled.

## Usage

Enable it in the [setup wizard](setup-and-configuration.md), set
`INSTALL_PASEO="true"` in
[config](setup-and-configuration.md#configuration-reference), or install it into
a running container from the [control panel](control-panel.md) → "Install paseo".

With paseo on:

- The panel's "Launch an installed agent" offers a **"via paseo"** mode for the
  natively-supported providers.
- The panel gains **"Show paseo pairing QR code"** — scan it to connect your
  phone.

Installed via `pnpm add -g @getpaseo/cli`.

## How it works

The paseo daemon dials **out** to the relay with end-to-end encryption, so
nothing is exposed inbound — the same no-inbound-port posture as
[Claude remote control](claude-remote-control.md). The daemon (started with the
pairing QR) supervises agents, and you orchestrate them from the paseo client;
`paseo run` headless isn't the intended path, so agents still launch directly in
their own tmux windows.

The host-side constants (relay host, install spec) are in
[agents.rs](../crates/introdus-core/src/agents.rs); the panel actions
(install, QR, daemon-ensure snippet) are in
[menu_actions.rs](../crates/introdus-cli/src/menu_actions.rs).
