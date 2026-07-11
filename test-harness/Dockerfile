# Test harness image: runs the FULL introdus experience under rootless
# podman-in-podman, so the real tmux + podman + egress-firewall flow can be
# driven and asserted. Built from the official podman-in-podman base, which
# preconfigures subuid/subgid, /etc/containers, and fuse-overlayfs for the
# unprivileged `podman` user.
#
# Build context is the REPO ROOT (so it can COPY the release binary):
#   podman build -t introdus-harness -f test-harness/Dockerfile .
FROM quay.io/podman/stable

# Tools the harness + driver need beyond podman itself. passt provides `pasta`
# (rootless networking); nftables/tinyproxy live INSIDE the ubuntu dev image,
# not here.
RUN dnf -y install tmux git openssh-clients passt iproute procps-ng \
    && dnf clean all

# The control-plane binary under test (built on the host by harness.sh).
COPY target/release/introdus /usr/local/bin/introdus
RUN chmod 0755 /usr/local/bin/introdus

# Driver scripts, one per milestone; harness.sh picks which to run.
COPY test-harness/driver-verify.sh /usr/local/bin/driver-verify.sh
RUN chmod 0755 /usr/local/bin/driver-verify.sh

# Run as the preconfigured rootless user — introdus refuses to run as root.
USER podman
WORKDIR /home/podman
ENV HOME=/home/podman
