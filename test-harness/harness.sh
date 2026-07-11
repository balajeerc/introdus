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
#   --privileged            the OUTER container only, so the INNER container can
#                           mount its own /proc, set up userns/netns, run nft.
#                           The inner podman still runs rootless as the `podman`
#                           user, so introdus's non-root preflight still holds and
#                           the egress firewall under test is unchanged. The
#                           harness is trusted test infra (full egress by design).
#   --device /dev/fuse      fuse-overlayfs storage driver
#   --device /dev/net/tun   pasta needs it to make the inner container's tap dev
#   storage volume          persist the inner podman graph (the built base image)
#                           across harness runs — otherwise every run rebuilds it
common_flags=(
    --rm
    --privileged
    --device /dev/fuse
    --device /dev/net/tun
    -v introdus-harness-storage:/home/podman/.local/share/containers
)

case "$milestone" in
    verify)
        echo "==> running milestone 1 (verify) in the harness"
        podman run "${common_flags[@]}" "$image" driver-verify.sh
        ;;
    launch)
        echo "==> running milestone 2 (launch: clone + serve) in the harness"
        podman run "${common_flags[@]}" "$image" driver-launch.sh
        ;;
    menu)
        echo "==> running milestone 3 (menu: drive the live control TUI) in the harness"
        podman run "${common_flags[@]}" "$image" driver-menu.sh
        ;;
    *)
        echo "unknown milestone: $milestone" >&2
        exit 2
        ;;
esac
