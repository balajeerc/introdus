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
COPY test-harness/driver-common.sh /usr/local/bin/driver-common.sh
COPY test-harness/driver-verify.sh /usr/local/bin/driver-verify.sh
COPY test-harness/driver-launch.sh /usr/local/bin/driver-launch.sh
COPY test-harness/driver-menu.sh /usr/local/bin/driver-menu.sh
COPY test-harness/driver-egress.sh /usr/local/bin/driver-egress.sh
COPY test-harness/driver-lifecycle.sh /usr/local/bin/driver-lifecycle.sh
COPY test-harness/driver-install.sh /usr/local/bin/driver-install.sh
COPY test-harness/driver-agents.sh /usr/local/bin/driver-agents.sh
COPY test-harness/driver-agent-launch.sh /usr/local/bin/driver-agent-launch.sh
RUN chmod 0755 /usr/local/bin/driver-common.sh /usr/local/bin/driver-verify.sh \
    /usr/local/bin/driver-launch.sh /usr/local/bin/driver-menu.sh \
    /usr/local/bin/driver-egress.sh /usr/local/bin/driver-lifecycle.sh \
    /usr/local/bin/driver-install.sh /usr/local/bin/driver-agents.sh \
    /usr/local/bin/driver-agent-launch.sh

# Run as the preconfigured rootless user — introdus refuses to run as root.
USER podman
WORKDIR /home/podman
ENV HOME=/home/podman
# The nested (inner) containers must get a PRIVATE uts namespace: introdus passes
# --hostname, which podman rejects under the host UTS ns that the podman-in-podman
# base otherwise defaults to. Private uts is the normal podman default anyway.
# default_sysctls is emptied because the built-in net.ipv4.ping_group_range
# default can't be written to the read-only /proc/sys when nested (crun errors).
RUN mkdir -p /home/podman/.config/containers \
    && printf '[containers]\nutsns = "private"\ndefault_sysctls = []\n' \
       > /home/podman/.config/containers/containers.conf
