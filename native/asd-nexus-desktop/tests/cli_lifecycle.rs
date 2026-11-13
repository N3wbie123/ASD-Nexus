//! Binary-level tests: CLI flags, non-interactive `--verify`, and the
//! sidecar lifecycle (spawn when `--server-url` is absent, terminate on exit).

use std::path::PathBuf;
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_asd-nexus-desktop"))
}

fn repo_root() -> PathBuf {
    // native/asd-nexus-desktop -> repo root
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

#[test]
fn help_and_bad_flags() {
    let out = bin().arg("--help").output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    for flag in ["--project-root", "--server-url", "--token", "--verify", "--screenshot"] {
        assert!(text.contains(flag), "help missing {flag}");
    }
    let out = bin().arg("--nope").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn verify_against_unreachable_server_fails_fast_without_window() {
    let start = std::time::Instant::now();
    let out = bin()
        .args(["--verify", "--server-url", "http://127.0.0.1:9", "--token", "t"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json report");
    assert_eq!(report["ok"], false);
    assert_eq!(report["spawnedSidecar"], false);
    assert_eq!(report["monochromeVisuals"], true);
    assert_eq!(report["tabsRendered"].as_array().unwrap().len(), 6);
    assert!(start.elapsed().as_secs() < 45);
}

fn node_available() -> bool {
    Command::new("node").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

#[cfg(windows)]
fn pid_alive(pid: u64) -> bool {
    let out = Command::new("tasklist").args(["/FI", &format!("PID eq {pid}"), "/NH"]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).contains(&pid.to_string())
}

#[cfg(not(windows))]
fn pid_alive(pid: u64) -> bool {
    Command::new("kill").args(["-0", &pid.to_string()]).status().map(|s| s.success()).unwrap_or(false)
}

/// Spawns the real Node sidecar (`--port 0`), renders all tabs headlessly
/// against it, and asserts the sidecar process is gone after exit.
/// Skipped when Node or `src/server/index.ts` is unavailable.
#[test]
fn verify_spawns_sidecar_and_terminates_it() {
    let root = repo_root();
    if !node_available() || !root.join("src").join("server").join("index.ts").exists() {
        eprintln!("skipping: node or src/server/index.ts not available");
        return;
    }
    let out = bin().arg("--verify").arg("--project-root").arg(&root).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("bad report ({e}): {stdout}"));
    assert_eq!(report["ok"], true, "verify failed: {stdout}");
    assert_eq!(report["spawnedSidecar"], true);
    assert_eq!(report["routingLoaded"], true);
    let pid = report["health"]["pid"].as_u64().expect("sidecar pid");
    assert!(!pid_alive(pid), "sidecar pid {pid} still alive after exit");
}
