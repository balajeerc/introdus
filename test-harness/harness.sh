#!/usr/bin/env bash
# Host-side entry point for the full-experience test harness. Builds the
# introdus release binary, bakes it into the podman-in-podman harness image, and
# runs a driver inside a rootless-in-rootless container.
#
# Usage:
#   test-harness/harness.sh [target]
#     verify     egress spike: nested base build + egress firewall self-check
#     launch     full dev container up + public-repo clone through the proxy
#     reattach   repeat launch from the same dir reattaches (dir tag > name hash)
#     menu       drive the live control TUI over tmux (terminals/copy/allowlist/
#                stop/restart)
#     egress     workload default-deny enforcement + blocked-egress menu utility
#     lifecycle  recreate (persistence) + destroy (confirm/scan/key) teardown
#     install    install a coding agent: live progress + menu disabled (no cascade)
#     agents     claude is opt-out: absent when unselected, installable on demand
#     agent-launch  launch an agent with its skip-permissions flag from the menu
#     agent-missing a selected-but-uninstalled agent is caught before launch
#     quit-stop  "Quit introdus (stop the container)" stops it + kills the session
#     detach     "Detach tmux session" detaches the client; session + container survive
#     paseo      install paseo, launch an agent directly, show the pairing QR
#     all        verify + menu + egress + lifecycle + install + agents +
#                agent-launch + agent-missing + quit-stop + detach +
#                paseo (default)
#
# This is a heavy, opt-in tier — it is NOT part of `cargo test`. It needs a
# rootless-podman host with /dev/fuse and /dev/net/tun.
set -euo pipefail

cd "$(dirname "$0")/.."

target="${1:-all}"
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

run_driver() { podman run "${common_flags[@]}" "$image" "$1"; }

case "$target" in
    verify)
        echo "==> verify: nested base build + egress self-check"
        run_driver driver-verify.sh
        ;;
    launch)
        echo "==> launch: full dev container up + clone through the proxy"
        run_driver driver-launch.sh
        ;;
    reattach)
        echo "==> reattach: repeat launch from the same dir reattaches to one session"
        run_driver driver-reattach.sh
        ;;
    menu)
        echo "==> menu: drive the live control TUI over tmux"
        run_driver driver-menu.sh
        ;;
    egress)
        echo "==> egress: workload default-deny enforcement + blocked-egress menu"
        run_driver driver-egress.sh
        ;;
    lifecycle)
        echo "==> lifecycle: recreate persistence + destroy teardown"
        run_driver driver-lifecycle.sh
        ;;
    install)
        echo "==> install: install a coding agent — live progress + no cascade"
        run_driver driver-install.sh
        ;;
    agents)
        echo "==> agents: claude opt-out (absent unselected) + opt-in (menu install)"
        run_driver driver-agents.sh
        ;;
    agent-launch)
        echo "==> agent-launch: launch an agent with its skip-permissions flag"
        run_driver driver-agent-launch.sh
        ;;
    agent-missing)
        echo "==> agent-missing: a selected-but-uninstalled agent is caught before launch"
        run_driver driver-agent-missing.sh
        ;;
    quit-stop)
        echo "==> quit-stop: Quit introdus stops the container + kills the session"
        run_driver driver-quit-stop.sh
        ;;
    detach)
        echo "==> detach: Detach tmux session detaches the client; session + container survive"
        run_driver driver-detach.sh
        ;;
    paseo)
        echo "==> paseo: install paseo, launch an agent directly, show the pairing QR"
        run_driver driver-paseo.sh
        ;;
    all)
        echo "==> verify: nested base build + egress self-check"
        run_driver driver-verify.sh
        echo "==> reattach: repeat launch from the same dir reattaches to one session"
        run_driver driver-reattach.sh
        echo "==> menu: full launch + drive the live control TUI over tmux"
        run_driver driver-menu.sh
        echo "==> egress: workload default-deny enforcement + blocked-egress menu"
        run_driver driver-egress.sh
        echo "==> install: install a coding agent — live progress + no cascade"
        run_driver driver-install.sh
        echo "==> agents: claude opt-out (absent unselected) + opt-in (menu install)"
        run_driver driver-agents.sh
        echo "==> agent-launch: launch an agent with its skip-permissions flag"
        run_driver driver-agent-launch.sh
        echo "==> agent-missing: a selected-but-uninstalled agent is caught before launch"
        run_driver driver-agent-missing.sh
        echo "==> quit-stop: Quit introdus stops the container + kills the session"
        run_driver driver-quit-stop.sh
        echo "==> detach: Detach tmux session detaches the client; session + container survive"
        run_driver driver-detach.sh
        echo "==> paseo: install paseo, launch an agent directly, show the pairing QR"
        run_driver driver-paseo.sh
        echo "==> lifecycle: recreate persistence + destroy teardown"
        run_driver driver-lifecycle.sh
        ;;
    *)
        echo "unknown target: $target (want: verify | launch | reattach | menu | egress | lifecycle | install | agents | agent-launch | agent-missing | quit-stop | detach | paseo | all)" >&2
        exit 2
        ;;
esac

echo
echo "==> harness target '$target' PASSED"
