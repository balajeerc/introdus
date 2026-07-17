#!/usr/bin/env bash
# Milestone 1 (feasibility spike): prove that, INSIDE this rootless
# podman-in-podman harness, introdus can build the ubuntu base image and stand
# up the egress-firewalled dev container far enough to pass its own self-check.
#
# `introdus verify` = image::ensure (build base if missing) + run the container
# with VERIFY_ONLY: it installs the nft default-deny filter, starts the
# hostname-allowlist proxy, confirms the proxy reaches an allowlisted host AND
# that a direct-IP dial is dropped, then exits without the workload.
# Covers TEST_PLAN: TA23, TA33, TA49, TA110
set -euo pipefail
source /usr/local/bin/driver-common.sh

proj="$HOME/proj-verify"
mkdir -p "$proj"

# A dummy deploy key so input validation passes; verify never actually clones.
mkdir -p "$HOME/.ssh"
[[ -f "$HOME/.ssh/harness-key" ]] || ssh-keygen -t ed25519 -N "" -C harness \
    -f "$HOME/.ssh/harness-key" >/dev/null

# REPO_URL host (github.com) is what the self-check probes through the proxy, so
# it must be an allowlisted, real, reachable host.
mkdir -p "$proj/.introdus"
cat > "$proj/.introdus/config.env" <<EOF
PROJECT_NAME=harness
REPO_URL=git@github.com:octocat/Hello-World.git
DEPLOY_KEY_PATH=$HOME/.ssh/harness-key
WEBAPP_PORT=3000
INSTALL_AGENTS="claude"
EOF

cd "$proj"

echo "==================================================================="
echo " podman info (rootless-in-rootless sanity)"
echo "==================================================================="
podman info --format '  rootless={{.Host.Security.Rootless}} graphDriver={{.Store.GraphDriverName}}'

echo
echo "==================================================================="
echo " introdus verify  (build base image + nested egress self-check)"
echo "==================================================================="
introdus verify

echo
echo "=== MILESTONE 1 OK: nested podman built the base image and the egress"
echo "    firewall self-check passed inside the dev container. ==="
