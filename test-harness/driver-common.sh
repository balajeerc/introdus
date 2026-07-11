#!/usr/bin/env bash
# Shared setup sourced by every milestone driver.

# logind isn't present to provision /run/user/<uid>. Give introdus (and the
# nested podman) an explicit XDG_RUNTIME_DIR under /tmp — /tmp supports the netns
# bind-mounts pasta needs, whereas the home overlay fs does not. Used for the
# notify FIFO, runtime state, and the nested podman's RunRoot/netns.
export XDG_RUNTIME_DIR="/tmp/xdg-$(id -u)"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"
