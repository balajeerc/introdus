//! Where a podman command runs: on this machine, or on a remote container-host
//! reached over `ssh`.
//!
//! [`Location`] centralizes the "run `podman …` here vs. over ssh" decision so
//! the `send-files` flow (host picker → container picker → transfer) can stay
//! location-agnostic. A [`Location::Remote`] wraps every invocation in
//! `ssh <alias> "<quoted podman command>"`; the whole podman command is passed
//! as a single, per-token shell-quoted argument so paths with spaces or
//! metacharacters survive the remote shell intact.
//!
//! ssh runs non-interactively (`BatchMode=yes`): this tool already assumes a
//! passwordless alias (same as `notify-listen`'s reverse tunnel), and a hidden
//! password prompt would silently hang a full-screen TUI. A short
//! `ConnectTimeout` keeps an unreachable host from stalling the picker.

use crate::process::{sh_quote, Cmd};

/// Non-interactive ssh options: fail instead of prompting for a password (which
/// a full-screen TUI can't show), and time out rather than hang on a dead host.
const SSH_OPTS: [&str; 4] = ["-o", "BatchMode=yes", "-o", "ConnectTimeout=10"];

/// Where to run `podman`: locally or on an ssh-reachable host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    /// This machine — run `podman` directly.
    Local,
    /// A remote host named by its `~/.ssh/config` alias — run `podman` over ssh.
    Remote(String),
}

impl Location {
    /// A human label for headers/pickers (`this machine` / the ssh alias).
    pub fn label(&self) -> &str {
        match self {
            Location::Local => "this machine",
            Location::Remote(alias) => alias,
        }
    }

    /// The full argv that runs `podman <args>` at this location. For
    /// [`Local`](Location::Local) it's `["podman", …args]`; for
    /// [`Remote`](Location::Remote) it's
    /// `["ssh", <opts>, <alias>, "podman <quoted args>"]` — the podman command
    /// collapsed into one shell-quoted string so the remote shell re-parses it
    /// as the intended tokens (not split on spaces in a path).
    pub fn podman_argv(&self, args: &[&str]) -> Vec<String> {
        match self {
            Location::Local => std::iter::once("podman")
                .chain(args.iter().copied())
                .map(str::to_owned)
                .collect(),
            Location::Remote(alias) => {
                let remote = std::iter::once("podman")
                    .chain(args.iter().copied())
                    .map(sh_quote)
                    .collect::<Vec<_>>()
                    .join(" ");
                let mut argv = vec!["ssh".to_owned()];
                argv.extend(SSH_OPTS.iter().map(|s| (*s).to_owned()));
                argv.push(alias.clone());
                argv.push(remote);
                argv
            }
        }
    }

    /// A [`Cmd`] running `podman <args>` at this location, built from
    /// [`podman_argv`](Location::podman_argv) so the two share one wrapping rule.
    pub fn podman(&self, args: &[&str]) -> Cmd {
        let argv = self.podman_argv(args);
        Cmd::new(&argv[0]).args(&argv[1..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ta143_local_argv_is_plain_podman() {
        let loc = Location::Local;
        assert_eq!(
            loc.podman_argv(&["ps", "--format", "{{.Names}}"]),
            vec!["podman", "ps", "--format", "{{.Names}}"]
        );
        assert_eq!(loc.label(), "this machine");
    }

    #[test]
    fn ta143_remote_argv_wraps_in_ssh_with_one_quoted_command() {
        let loc = Location::Remote("devbox".to_owned());
        let argv = loc.podman_argv(&["exec", "cx", "ls", "-1Ap", "--", "/home/dev"]);
        assert_eq!(
            argv,
            vec![
                "ssh",
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=10",
                "devbox",
                // The whole podman command is ONE arg, each token single-quoted.
                "'podman' 'exec' 'cx' 'ls' '-1Ap' '--' '/home/dev'",
            ]
        );
        assert_eq!(loc.label(), "devbox");
    }

    #[test]
    fn ta143_remote_quoting_survives_spaces_and_metachars() {
        // A destination path with a space (and a literal quote) must stay a
        // single token on the remote side, not split or terminate the quoting.
        let loc = Location::Remote("h".to_owned());
        let argv = loc.podman_argv(&["cp", "-", "cx:/home/dev/my docs"]);
        let remote = argv.last().unwrap();
        assert_eq!(remote, "'podman' 'cp' '-' 'cx:/home/dev/my docs'");
        assert_eq!(
            loc.podman_argv(&["exec", "cx", "echo", "a'b"])
                .last()
                .unwrap(),
            r"'podman' 'exec' 'cx' 'echo' 'a'\''b'"
        );
    }

    #[test]
    fn ta143_podman_cmd_label_matches_argv() {
        // The Cmd built from the argv logs the same command line.
        assert_eq!(
            Location::Local.podman(&["cp", "-", "cx:/d"]).label(),
            "podman cp - cx:/d"
        );
    }
}
