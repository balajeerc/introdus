//! Building and launching the container: the full `podman run` flag/env/mount
//! set (parity with `launch_dev_container.sh`), plus the `--verify` throwaway
//! self-check and the `--update` in-container refresh.

use std::convert::Infallible;

use anyhow::{bail, Context, Result};
use introdus_core::podman::{self, podman};
use introdus_core::ports::parse_extra_ports;

use crate::context::LaunchContext;

/// Capabilities added back on top of `--cap-drop=ALL` (NET_ADMIN is added
/// separately, only when the egress filter is on).
const CAP_ADD: &[&str] = &[
    "CHOWN",
    "DAC_OVERRIDE",
    "FOWNER",
    "FSETID",
    "SETFCAP",
    "MKNOD",
    "SETUID",
    "SETGID",
];

/// Validate the launch inputs the shell checked at env-parse time: the deploy
/// key exists, a shared-data path (if set) is a directory, extra ports parse.
pub fn validate_inputs(ctx: &LaunchContext) -> Result<()> {
    if !std::path::Path::new(&ctx.config.deploy_key_path).is_file() {
        bail!(
            "DEPLOY_KEY_PATH does not exist: {}",
            ctx.config.deploy_key_path
        );
    }
    if let Some(p) = &ctx.config.shared_data_path {
        if !std::path::Path::new(p).is_dir() {
            bail!("SHARED_DATA_PATH is not a directory: {p}");
        }
    }
    parse_extra_ports(&ctx.config.extra_ports, ctx.config.webapp_port)?;
    Ok(())
}

/// Append a literal argument.
fn lit(a: &mut Vec<String>, s: &str) {
    a.push(s.to_owned());
}

/// Append a `--volume host:dest` pair.
fn vol(a: &mut Vec<String>, host: &str, dest: &str) {
    a.push("--volume".to_owned());
    a.push(format!("{host}:{dest}"));
}

/// Append a `--env KEY=VALUE` pair.
fn env(a: &mut Vec<String>, key: &str, value: String) {
    a.push("--env".to_owned());
    a.push(format!("{key}={value}"));
}

/// Build the argument vector after `podman` for creating the container.
pub fn run_args(ctx: &LaunchContext, disable_network_block: bool) -> Result<Vec<String>> {
    let c = &ctx.config;
    let mut a: Vec<String> = Vec::new();

    lit(&mut a, "run");
    lit(&mut a, "-it");
    lit(&mut a, "--name");
    a.push(ctx.container_name.clone());
    lit(&mut a, "--hostname");
    lit(&mut a, "introdus");
    lit(&mut a, "--network=pasta");
    a.push(format!("--memory={}", c.mem_limit));
    a.push(format!("--cpus={}", c.cpu_limit));
    a.push(format!("--pids-limit={}", c.pids_limit));
    lit(&mut a, "--cap-drop=ALL");
    for cap in CAP_ADD {
        a.push(format!("--cap-add={cap}"));
    }
    if !disable_network_block {
        lit(&mut a, "--cap-add=NET_ADMIN");
    }
    lit(&mut a, "--security-opt=no-new-privileges");

    push_mounts(ctx, &mut a)?;
    push_env(ctx, disable_network_block, &mut a);
    push_publish(ctx, &mut a)?;

    a.push(ctx.image_name.clone());
    lit(&mut a, "/usr/local/bin/firewall-entrypoint.sh");
    Ok(a)
}

fn push_mounts(ctx: &LaunchContext, a: &mut Vec<String>) -> Result<()> {
    vol(a, &ctx.volume_name, "/home/dev");
    vol(a, &ctx.config.deploy_key_path, "/tmp/deploy_key:ro");
    vol(a, &path_str(&ctx.setup_script())?, "/setup.sh:ro");
    vol(
        a,
        &path_str(&ctx.entrypoint())?,
        "/usr/local/bin/firewall-entrypoint.sh:ro",
    );
    vol(
        a,
        &path_str(&ctx.tinyproxy_conf())?,
        "/etc/tinyproxy/tinyproxy.conf:ro",
    );
    vol(
        a,
        &path_str(&ctx.allowlist_file)?,
        "/etc/tinyproxy/egress-allowlist.txt:ro",
    );
    if let Some(shared) = &ctx.config.shared_data_path {
        let canon = std::fs::canonicalize(shared)
            .with_context(|| format!("resolving SHARED_DATA_PATH {shared}"))?;
        vol(a, &path_str(&canon)?, "/home/dev/shared_data:ro");
    }
    // Notification endpoint: ensure the host FIFO exists and bind-mount it at
    // /run/notify so the container's rc-notify hook can deliver events to the
    // `introdus notify-host` service running in the session's notify window.
    let fifo = crate::notify::fifo_path()?;
    crate::notify::ensure_fifo(&fifo)?;
    vol(a, &path_str(&fifo)?, "/run/notify");
    Ok(())
}

fn push_env(ctx: &LaunchContext, disable_network_block: bool, a: &mut Vec<String>) {
    let c = &ctx.config;
    env(a, "PROJECT_NAME", c.project_name.clone());
    env(a, "CONTAINER_NAME", ctx.container_name.clone());
    env(a, "REPO_URL", c.repo_url.clone());
    env(a, "WEBAPP_PORT", c.webapp_port.to_string());
    env(
        a,
        "ON_LAUNCH_SCRIPT",
        c.on_launch_script.clone().unwrap_or_default(),
    );
    env(
        a,
        "ON_LAUNCH_ROOT_SCRIPT",
        c.on_launch_root_script.clone().unwrap_or_default(),
    );
    env(
        a,
        "ON_LAUNCH_ROOT_TIMEOUT",
        c.on_launch_root_timeout.to_string(),
    );
    env(a, "CANARY_BLOCKED_IP", c.canary_blocked_ip.clone());
    env(a, "HOST_OS", "linux".to_owned());
    env(
        a,
        "DISABLE_NETWORK_BLOCK",
        disable_network_block.to_string(),
    );
    env(a, "EXPOSE_WEBAPP", c.expose_webapp.to_string());
    env(a, "TUNNEL_EDGE_IPS", ctx.tunnel_edge_ips.join(" "));
    env(a, "TUNNEL_API_IPS", ctx.tunnel_api_ips.join(" "));
    env(a, "WHITELIST_HOSTS", ctx.container_whitelist.join(" "));
    env(a, "INTERNAL_ALLOW_CIDRS", c.internal_allow_cidrs.join(" "));
    env(
        a,
        "ENABLE_NOTIFY_SH_ALERTS",
        c.enable_notify_sh_alerts.to_string(),
    );
    env(
        a,
        "NTFY_SH_TOPIC",
        c.ntfy_sh_topic.clone().unwrap_or_default(),
    );
    env(a, "INSTALL_AGENTS", c.install_agents.join(" "));
}

fn push_publish(ctx: &LaunchContext, a: &mut Vec<String>) -> Result<()> {
    let port = ctx.config.webapp_port;
    a.push("--publish".to_owned());
    a.push(format!("127.0.0.1:{port}:{port}"));
    for (host, container) in parse_extra_ports(&ctx.config.extra_ports, port)? {
        a.push("--publish".to_owned());
        a.push(format!("127.0.0.1:{host}:{container}"));
    }
    Ok(())
}

fn path_str(p: &std::path::Path) -> Result<String> {
    p.to_str()
        .map(str::to_owned)
        .with_context(|| format!("path is not valid UTF-8: {}", p.display()))
}

/// Create a fresh container and hand the terminal to it (never returns on
/// success). The caller has already ensured the image, volume, and allowlist.
pub fn create_and_exec(ctx: &LaunchContext, disable_network_block: bool) -> Result<Infallible> {
    println!("==> creating new container {}", ctx.container_name);
    let argv = run_args(ctx, disable_network_block)?;
    podman().args(argv).exec()
}

/// Start (and attach to) an already-created container.
pub fn start_and_exec(ctx: &LaunchContext) -> Result<Infallible> {
    println!(
        "==> reusing existing container {} (recreate/reset to rebuild it)",
        ctx.container_name
    );
    podman().args(["start", "-ai", &ctx.container_name]).exec()
}

/// `introdus verify`: run the firewall self-check in a throwaway container.
pub fn verify(ctx: &LaunchContext) -> Result<()> {
    println!("==> verify: running egress filter + proxy self-check in a throwaway container");
    ctx.write_allowlist()?;
    podman()
        .args(["run", "--rm", "--cap-drop=ALL"])
        .args([
            "--cap-add=CHOWN",
            "--cap-add=DAC_OVERRIDE",
            "--cap-add=FOWNER",
        ])
        .args([
            "--cap-add=SETUID",
            "--cap-add=SETGID",
            "--cap-add=NET_ADMIN",
        ])
        .args(["--security-opt=no-new-privileges", "--network=pasta"])
        .args(["--env", "VERIFY_ONLY=true"])
        .args([
            "--env",
            &format!("WHITELIST_HOSTS={}", ctx.container_whitelist.join(" ")),
        ])
        .args([
            "--env",
            &format!(
                "INTERNAL_ALLOW_CIDRS={}",
                ctx.config.internal_allow_cidrs.join(" ")
            ),
        ])
        .args([
            "--env",
            &format!("TUNNEL_EDGE_IPS={}", ctx.tunnel_edge_ips.join(" ")),
        ])
        .args([
            "--env",
            &format!("TUNNEL_API_IPS={}", ctx.tunnel_api_ips.join(" ")),
        ])
        .args([
            "--env",
            &format!("CANARY_BLOCKED_IP={}", ctx.config.canary_blocked_ip),
        ])
        .args(["--env", &format!("REPO_URL={}", ctx.config.repo_url)])
        .arg("--volume")
        .arg(format!(
            "{}:/usr/local/bin/firewall-entrypoint.sh:ro",
            path_str(&ctx.entrypoint())?
        ))
        .arg("--volume")
        .arg(format!(
            "{}:/etc/tinyproxy/tinyproxy.conf:ro",
            path_str(&ctx.tinyproxy_conf())?
        ))
        .arg("--volume")
        .arg(format!(
            "{}:/etc/tinyproxy/egress-allowlist.txt:ro",
            path_str(&ctx.allowlist_file)?
        ))
        .arg(&ctx.image_name)
        .arg("/usr/local/bin/firewall-entrypoint.sh")
        .run()?;
    println!("==> verify passed");
    Ok(())
}

/// `introdus update`: in-container refresh (apt, mise, agents, LazyVim) against
/// a running container. Requires the container to be up (it routes through the
/// egress filter the entrypoint installed).
pub fn update(ctx: &LaunchContext) -> Result<()> {
    if !podman::container_running(&ctx.container_name) {
        bail!(
            "container {} is not running. launch it first.",
            ctx.container_name
        );
    }
    println!("==> update: apt upgrade (as root, via the proxy)");
    podman::exec(&ctx.container_name, None)
        .args(["bash", "-c", APT_UPGRADE])
        .run()?;
    println!("==> update: mise / agents / lazyvim (as dev)");
    podman::exec(&ctx.container_name, Some("dev"))
        .env("INSTALL_AGENTS", ctx.config.install_agents.join(" "))
        .args(["bash", "-c", DEV_UPDATE])
        .run()?;
    println!("==> update: done");
    Ok(())
}

const APT_UPGRADE: &str = "set -e; export DEBIAN_FRONTEND=noninteractive; \
     apt-get update && apt-get -y upgrade";

const DEV_UPDATE: &str = r#"set -e
export HOME=/home/dev
export PATH="/home/dev/.local/bin:/home/dev/.local/share/mise/shims:/home/dev/.local/share/pnpm/bin:$PATH"
eval "$(/home/dev/.local/bin/mise activate bash)"
mise self-update -y || true
mise upgrade
pnpm update -g @anthropic-ai/claude-code
node "$(pnpm root -g)/@anthropic-ai/claude-code/install.cjs"
[ -x /usr/local/bin/install-agents ] && /usr/local/bin/install-agents || true
if [ -f /usr/local/lib/rc-agents.sh ]; then
  . /usr/local/lib/rc-agents.sh
  for _id in ${INSTALL_AGENTS:-claude}; do
    [ "$_id" = claude ] && continue
    [ "${AGENT_METHOD[$_id]:-}" = pnpm ] || continue
    pnpm update -g --ignore-scripts "${AGENT_SPEC[$_id]}" || true
  done
fi
nvim --headless "+Lazy! sync" +qa"#;

#[cfg(test)]
mod tests {
    use super::*;
    use introdus_core::Config;

    fn ctx() -> LaunchContext {
        let mut cfg = Config::new(
            "web".to_owned(),
            "git@github.com:o/r.git".to_owned(),
            "/dev/null".to_owned(), // exists as a file for validate_inputs
            3000,
        );
        cfg.image_suffix = Some("ab12".to_owned());
        cfg.extra_ports = vec!["8123".to_owned()];
        LaunchContext::resolve(cfg, std::env::temp_dir()).unwrap()
    }

    #[test]
    fn run_args_have_the_hardening_flags() {
        let a = run_args(&ctx(), false).unwrap();
        assert!(a.contains(&"--cap-drop=ALL".to_owned()));
        assert!(a.contains(&"--cap-add=NET_ADMIN".to_owned()));
        assert!(a.contains(&"--security-opt=no-new-privileges".to_owned()));
        assert!(a.contains(&"--network=pasta".to_owned()));
        // ends with the entrypoint after the image
        let img = a
            .iter()
            .position(|s| s == "introdus-web-ab12:latest")
            .unwrap();
        assert_eq!(a[img + 1], "/usr/local/bin/firewall-entrypoint.sh");
    }

    #[test]
    fn disable_network_block_drops_net_admin() {
        let a = run_args(&ctx(), true).unwrap();
        assert!(!a.contains(&"--cap-add=NET_ADMIN".to_owned()));
        assert!(a.iter().any(|s| s == "DISABLE_NETWORK_BLOCK=true"));
    }

    #[test]
    fn publishes_webapp_and_extra_ports() {
        let a = run_args(&ctx(), false).unwrap();
        assert!(a.contains(&"127.0.0.1:3000:3000".to_owned()));
        assert!(a.contains(&"127.0.0.1:8123:8123".to_owned()));
    }
}
