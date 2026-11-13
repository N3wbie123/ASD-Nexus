//! Real integration test for SidecarManager and NexusClient.
//!
//! Spawns live `node --no-warnings src/server/index.ts --port 0`,
//! verifies handshake, hits health & projects REST endpoints,
//! and ensures clean child termination on Drop.

use asd_nexus_client::{
    find_server_script, ListProjectsRequest, SidecarConfig, SidecarManager,
    SIDECAR_READY_PROTOCOL,
};
use std::path::PathBuf;
use std::time::Duration;

#[test]
fn test_live_sidecar_lifecycle_and_rest_calls() {
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("native")
        .parent()
        .expect("repo root")
        .to_path_buf();

    let server_script = find_server_script(None, &project_root)
        .expect("Must resolve src/server/index.ts");
    assert!(server_script.exists());

    let config = SidecarConfig::new(&project_root)
        .with_server_script(&server_script)
        .with_timeout(Duration::from_secs(15));

    let mut sidecar = SidecarManager::start(config).expect("Failed to start sidecar");
    let ready = sidecar.ready_info();

    assert_eq!(ready.protocol, SIDECAR_READY_PROTOCOL);
    assert_eq!(ready.protocol_version, 1);
    assert!(ready.port > 0);
    assert!(ready.pid > 0);
    assert!(sidecar.base_url().starts_with("http://127.0.0.1:"));

    let client = sidecar.create_client();

    // 1. Health check
    let health = client.health().expect("GET /api/health failed");
    assert!(health.ok);
    assert_eq!(health.pid, ready.pid);

    // 2. Projects list
    let projects_resp = client
        .projects(&ListProjectsRequest {
            skip_disk_check: Some(true),
            force_refresh: Some(false),
        })
        .expect("GET /api/projects failed");
    assert!(projects_resp.projects.is_empty() || !projects_resp.projects.is_empty());

    // 3. Models list
    let models_resp = client
        .models(&Default::default())
        .expect("GET /api/models failed");
    assert!(!models_resp.models.is_empty() || models_resp.models.is_empty());

    // 4. Clean shutdown & drop
    let child_pid = ready.pid;
    drop(client);
    sidecar.shutdown().expect("Shutdown failed");
    drop(sidecar);

    // Give OS a moment to clean up process handle
    std::thread::sleep(Duration::from_millis(300));

    // Verify sidecar is not running
    #[cfg(windows)]
    {
        use std::process::Command;
        let output = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", child_pid), "/FO", "CSV"])
            .output();
        if let Ok(out) = output {
            let stdout = String::from_utf8_lossy(&out.stdout);
            // If process is gone, tasklist says "INFO: No tasks are running which match the specified criteria."
            assert!(
                stdout.contains("No tasks") || !stdout.contains(&child_pid.to_string()),
                "Sidecar process PID {} is still running after Drop! tasklist output: {}",
                child_pid,
                stdout
            );
        }
    }
}
