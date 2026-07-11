//! Opt-in bubblewrap sandbox for agent runs. Confines a run's backend process so
//! an injected instruction that lands becomes a bad transcript, not an escape:
//! the workspace is bound read-write, the rest of the host filesystem is read-only,
//! `/tmp` is private, and — only when asked — the network is fully isolated.
//!
//! This module just builds the `bwrap` invocation and probes availability; wiring
//! it into the backend spawn (behind an opt-in, off by default) is a later slice.
//! See docs/run-sandbox-design.md for the rationale, including why network stays on
//! by default (`--unshare-net` cuts localhost too, which breaks every model API).

use std::path::PathBuf;
use std::process::Command;

/// What the sandbox confines a run to.
#[derive(Clone, Debug)]
pub struct SandboxPolicy {
    /// Bound read-write; the rest of the host filesystem is exposed read-only.
    pub workspace: PathBuf,
    /// Extra host paths to expose read-only (provider auth/config, the resolved
    /// backend binary's directory, …). Missing paths are tolerated.
    pub extra_ro_binds: Vec<PathBuf>,
    /// Keep the network (`true`) or fully isolate it with `--unshare-net`
    /// (`false`). Kept by default — see the module docs.
    pub net: bool,
}

impl SandboxPolicy {
    /// A filesystem-confining policy for `workspace`, with the network kept.
    pub fn new(workspace: impl Into<PathBuf>) -> Self {
        SandboxPolicy {
            workspace: workspace.into(),
            extra_ro_binds: Vec::new(),
            net: true,
        }
    }
}

/// True when bubblewrap can be used here: Linux with a working `bwrap` on PATH.
pub fn available() -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }
    Command::new("bwrap")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Wrap a program in a bubblewrap sandbox. Returns a `Command` whose program is
/// `bwrap` and whose trailing arguments end with `-- <bin>`, so the caller appends
/// the program's own arguments exactly as before (they become `bin`'s args). The
/// workspace is bound read-write at the same path in and out (no path translation),
/// the system is read-only, `/tmp` is private, and the network is isolated only when
/// `policy.net` is false.
pub fn wrap(bin: &str, policy: &SandboxPolicy) -> Command {
    let ws = policy.workspace.as_os_str();
    let mut cmd = Command::new("bwrap");
    cmd.arg("--die-with-parent").args([
        "--unshare-user",
        "--unshare-ipc",
        "--unshare-pid",
        "--unshare-uts",
        "--unshare-cgroup",
    ]);
    if !policy.net {
        cmd.arg("--unshare-net");
    }
    // Read-only system: /usr is required; the rest are ro-bind-try because they are
    // often symlinks into /usr or absent on a given distro.
    cmd.args(["--ro-bind", "/usr", "/usr"]);
    for p in ["/bin", "/sbin", "/lib", "/lib64", "/lib32", "/etc"] {
        cmd.args(["--ro-bind-try", p, p]);
    }
    cmd.args(["--proc", "/proc"])
        .args(["--dev", "/dev"])
        .args(["--tmpfs", "/tmp"]);
    // Workspace: read-write, same path inside and out.
    cmd.arg("--bind").arg(ws).arg(ws);
    // Extra read-only exposures (auth/config, binary dir).
    for p in &policy.extra_ro_binds {
        cmd.arg("--ro-bind-try").arg(p).arg(p);
    }
    // Run inside the workspace, then hand off to the program.
    cmd.arg("--chdir").arg(ws);
    cmd.arg("--").arg(bin);
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(cmd: &Command) -> Vec<String> {
        cmd.get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    /// True if `seq` appears as a contiguous run inside `args`.
    fn contains_seq(args: &[String], seq: &[&str]) -> bool {
        args.windows(seq.len())
            .any(|w| w.iter().zip(seq).all(|(a, b)| a == b))
    }

    #[test]
    fn wrap_builds_a_confined_argv() {
        let policy = SandboxPolicy {
            workspace: PathBuf::from("/home/u/proj"),
            extra_ro_binds: vec![PathBuf::from("/home/u/.claude")],
            net: true,
        };
        let cmd = wrap("claude", &policy);
        assert_eq!(cmd.get_program(), "bwrap");
        let args = argv(&cmd);

        // workspace bound read-write, same path in and out
        assert!(contains_seq(
            &args,
            &["--bind", "/home/u/proj", "/home/u/proj"]
        ));
        // a system root is read-only
        assert!(contains_seq(&args, &["--ro-bind", "/usr", "/usr"]));
        // the extra provider-config path is exposed read-only
        assert!(contains_seq(
            &args,
            &["--ro-bind-try", "/home/u/.claude", "/home/u/.claude"]
        ));
        // runs inside the workspace
        assert!(contains_seq(&args, &["--chdir", "/home/u/proj"]));
        // net kept → no --unshare-net
        assert!(!args.iter().any(|a| a == "--unshare-net"));

        // the program is last, immediately after `--`, with nothing after it
        // (the caller appends the program's own args)
        let dd = args.iter().position(|a| a == "--").expect("`--` present");
        assert_eq!(args[dd + 1], "claude");
        assert_eq!(dd + 2, args.len());
    }

    #[test]
    fn wrap_isolates_network_only_when_requested() {
        let mut policy = SandboxPolicy::new("/w");
        assert!(policy.net); // kept by default
        assert!(!argv(&wrap("sh", &policy))
            .iter()
            .any(|a| a == "--unshare-net"));

        policy.net = false;
        assert!(argv(&wrap("sh", &policy))
            .iter()
            .any(|a| a == "--unshare-net"));
    }
}
