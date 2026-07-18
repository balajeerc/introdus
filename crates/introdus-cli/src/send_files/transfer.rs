//! The actual send: stream a host file/folder into a running container's
//! directory, then hand ownership to `dev`.
//!
//! One code path for local and remote. The source is packed with `tar` on this
//! machine and piped into `podman cp -` (which extracts a stdin tarball into the
//! destination directory). For a [`Location::Remote`] the sink is just the
//! ssh-wrapped `podman cp -`, so the tar stream flows over ssh with no staging
//! file on the far side to clean up. A final `chown -R dev:dev` (as root in the
//! container) makes the delivered tree `dev`-owned, mirroring the host-menu
//! `copy_file` recipe.

use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

use introdus_core::remote::Location;

/// Send `src` (a host file or folder) into `dest_dir` inside `container` at
/// `loc`. Blocking — the caller runs it on a worker thread behind a spinner.
pub fn send(loc: &Location, container: &str, src: &Path, dest_dir: &str) -> Result<()> {
    let base = src
        .file_name()
        .and_then(|s| s.to_str())
        .with_context(|| format!("can't send {} (no file name)", src.display()))?;
    let parent = src.parent().unwrap_or_else(|| Path::new("."));

    stream_in(loc, container, parent, base, dest_dir)?;
    chown(loc, container, dest_dir, base)?;
    Ok(())
}

/// `tar -C <parent> -cf - <base>` piped into `podman cp - <container>:<dest>`.
fn stream_in(
    loc: &Location,
    container: &str,
    parent: &Path,
    base: &str,
    dest_dir: &str,
) -> Result<()> {
    let sink_target = format!("{container}:{dest_dir}");
    let sink_argv = loc.podman_argv(&["cp", "-", &sink_target]);

    // tar's stderr is dropped (a full-screen TUI owns the terminal); a failure
    // still shows up as a non-zero exit. Its stdout is the archive we pipe on.
    let mut tar = Command::new("tar")
        .arg("-C")
        .arg(parent)
        .args(["-cf", "-", base])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to spawn tar")?;
    let tar_out = tar.stdout.take().expect("tar stdout piped");

    // The sink's stdout is discarded (podman cp is quiet on success); its stderr
    // is captured so a real failure (ssh unreachable, no such dir) is reportable
    // rather than smeared across the alternate screen.
    let sink = Command::new(&sink_argv[0])
        .args(&sink_argv[1..])
        .stdin(tar_out)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn `{}`", sink_argv.join(" ")))?;
    let out = sink.wait_with_output().context("waiting on podman cp")?;
    let tar_ok = tar.wait().map(|s| s.success()).unwrap_or(false);

    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!("copy into container failed: {}", err.trim());
    }
    if !tar_ok {
        bail!("reading {} failed (tar exited non-zero)", base);
    }
    Ok(())
}

/// `chown -R dev:dev <dest_dir>/<base>` as root inside the container.
fn chown(loc: &Location, container: &str, dest_dir: &str, base: &str) -> Result<()> {
    let target = chown_target(dest_dir, base);
    loc.podman(&["exec", container, "chown", "-R", "dev:dev", &target])
        .stdout_quiet()
        .with_context(|| format!("chown {target} to dev failed"))?;
    Ok(())
}

/// The delivered entry's path inside the container: `<dest_dir>/<base>` with
/// exactly one separator, so a `dest_dir` of `/` yields `/base` (not `//base`).
fn chown_target(dest_dir: &str, base: &str) -> String {
    format!("{}/{}", dest_dir.trim_end_matches('/'), base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ta146_chown_target_uses_single_separator() {
        assert_eq!(chown_target("/home/dev", "file.txt"), "/home/dev/file.txt");
        assert_eq!(chown_target("/home/dev/", "file.txt"), "/home/dev/file.txt");
        // A root destination must not double the slash.
        assert_eq!(chown_target("/", "file.txt"), "/file.txt");
    }

    #[test]
    fn ta146_sink_argv_is_podman_cp_from_stdin() {
        // The tar stream is piped into exactly this argv (local form).
        let argv = Location::Local.podman_argv(&["cp", "-", "cx:/home/dev"]);
        assert_eq!(argv, vec!["podman", "cp", "-", "cx:/home/dev"]);
        // Remote form wraps the same in ssh, as one quoted command.
        let r = Location::Remote("h".into()).podman_argv(&["cp", "-", "cx:/home/dev"]);
        assert_eq!(r[0], "ssh");
        assert_eq!(r.last().unwrap(), "'podman' 'cp' '-' 'cx:/home/dev'");
    }
}
