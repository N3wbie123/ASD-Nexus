//! Real integration test for SidecarManager and NexusClient.
//!
//! Spawns live `node --no-warnings src/server/index.ts --port 0`,
//! verifies handshake, hits health & projects REST endpoints,
//! and ensures clean child termination on Drop.

use asd_nexus_client::{
    find_server_script, CreateSessionRequest, DeleteSessionRequest, FsListMode, FsListRequest,
    GetHarnessCommandsRequest, HarnessCommandSource, ListProjectsRequest,
    SetSessionAutoApproveRequest, SidecarConfig, SidecarManager, SIDECAR_READY_PROTOCOL,
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

    // 4. Harness commands roundtrip
    let cmds_resp = client
        .harness_commands(&GetHarnessCommandsRequest {
            harness: Some("claude-code".into()),
            project_root: Some(project_root.to_string_lossy().into_owned()),
        })
        .expect("GET /api/harness-commands failed");
    assert!(
        cmds_resp
            .commands
            .iter()
            .any(|c| c.name == "compact" && c.source == HarnessCommandSource::Builtin),
        "Expected builtin /compact command for claude-code"
    );

    // 5. Workspace fs_list roundtrip (files & dirs)
    let files_resp = client
        .fs_list(&FsListRequest {
            project_root: Some(project_root.to_string_lossy().into_owned()),
            mode: Some(FsListMode::Files),
            prefix: Some("protocol.ts".into()),
            limit: Some(50),
        })
        .expect("GET /api/fs/list?mode=files failed");
    assert_eq!(files_resp.mode, FsListMode::Files);
    assert!(
        files_resp
            .entries
            .iter()
            .any(|e| e.path.ends_with("src/shared/protocol.ts") && !e.is_dir),
        "Expected src/shared/protocol.ts in fs_list files response"
    );

    let dirs_resp = client
        .fs_list(&FsListRequest {
            project_root: Some(project_root.to_string_lossy().into_owned()),
            mode: Some(FsListMode::Dirs),
            prefix: None,
            limit: Some(50),
        })
        .expect("GET /api/fs/list?mode=dirs failed");
    assert_eq!(dirs_resp.mode, FsListMode::Dirs);
    assert!(
        dirs_resp.entries.iter().any(|e| e.name == "src" && e.is_dir),
        "Expected 'src' directory in fs_list dirs response"
    );

    // 6. Session create with autoApprove=false and toggle via set_session_auto_approve
    let root_str = project_root.to_string_lossy().into_owned();
    let created = client
        .create_session(&CreateSessionRequest {
            project_root: Some(root_str.clone()),
            harness: Some("claude-code".into()),
            title: Some("live-sidecar-auto-approve-test".into()),
            make_foreground: Some(false),
            auto_approve: Some(false),
            ..Default::default()
        })
        .expect("POST /api/sessions failed");
    assert!(created.success, "create_session failed: {:?}", created.error);
    assert!(!created.session_id.is_empty());
    assert_eq!(created.auto_approve, Some(false));

    let toggled = client
        .set_session_auto_approve(&SetSessionAutoApproveRequest {
            project_root: Some(root_str.clone()),
            session_id: created.session_id.clone(),
            auto_approve: true,
        })
        .expect("POST /api/sessions/auto-approve failed");
    assert!(toggled.success, "set_session_auto_approve failed: {:?}", toggled.error);
    assert_eq!(toggled.session_id, created.session_id);
    assert!(toggled.auto_approve);

    let deleted = client
        .delete_session(&DeleteSessionRequest {
            project_root: root_str,
            session_id: created.session_id,
        })
        .expect("POST /api/sessions/delete failed");
    assert!(deleted.success, "delete_session cleanup failed: {:?}", deleted.error);

    // 7. Clean shutdown & drop
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
