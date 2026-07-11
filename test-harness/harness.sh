#!/usr/bin/env bash
# Host-side entry point for the full-experience test harness. Builds the
# introdus release binary, bakes it into the podman-in-podman harness image, and
# runs a milestone driver inside a rootless-in-rootless container.
#
# Usage:
#   test-harness/harness.sh [verify]      # milestone 1 (default): egress spike
#
# This is a heavy, opt-in tier — it is NOT part of `cargo test`.
set -euo pipefail

cd "$(dirname "$0")/.."

milestone="${1:-verify}"
image="introdus-harness:latest"

echo "==> building introdus release binary"
cargo build --release

echo "==> building harness image ($image)"
podman build -t "$image" -f test-harness/Dockerfile .

# Flags for rootless podman-in-podman:
#   --device /dev/fuse      fuse-overlayfs storage driver
#   --device /dev/net/tun   pasta needs it to make the inner container's tap dev
#   label/seccomp off       so the nested container can set up its own userns+netns
#   storage volume          persist the inner podman graph (the built base image)
#                           across harness runs — otherwise every run rebuilds it
# We run as the image's unprivileged `podman` user (introdus refuses root), NOT
# --privileged.
common_flags=(
    --rm
    --device /dev/fuse
    --device /dev/net/tun
    --security-opt label=disable
    --security-opt seccomp=unconfined
    -v introdus-harness-storage:/home/podman/.local/share/containers
)

case "$milestone" in
    verify)
        echo "==> running milestone 1 (verify) in the harness"
        podman run "${common_flags[@]}" "$image" driver-verify.sh
        ;;
    *)
        echo "unknown milestone: $milestone" >&2
        exit 2
        ;;
esac
