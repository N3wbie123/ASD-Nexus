//! Sidecar process lifecycle management.
//!
//! Spawns and manages the Node.js ASD-Nexus sidecar server (`src/server/index.ts`),
//! handles the `--port 0` handshake, and guarantees clean child process termination
//! on Drop without orphaned `node.exe` instances.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use crate::client::NexusClient;
use crate::error::{NexusError, Result};
use crate::models::{SidecarReadyInfo, SIDECAR_READY_PROTOCOL};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Configuration for launching or connecting to the ASD-Nexus sidecar server.
#[derive(Debug, Clone)]
pub struct SidecarConfig {
    pub project_root: PathBuf,
    pub home: Option<PathBuf>,
    pub token: Option<String>,
    pub node_bin: Option<PathBuf>,
    pub server_script: Option<PathBuf>,
    pub external_url: Option<String>,
    pub host: String,
    pub port: u16,
    pub timeout: Duration,
}

impl Default for SidecarConfig {
    fn default() -> Self {
        Self {
            project_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            home: None,
            token: None,
            node_bin: None,
            server_script: None,
            external_url: None,
            host: "127.0.0.1".into(),
            port: 0,
            timeout: Duration::from_secs(30),
        }
    }
}

impl SidecarConfig {
    pub fn new<P: AsRef<Path>>(project_root: P) -> Self {
        Self {
            project_root: project_root.as_ref().to_path_buf(),
            ..Default::default()
        }
    }

    pub fn with_home<P: AsRef<Path>>(mut self, home: P) -> Self {
        self.home = Some(home.as_ref().to_path_buf());
        self
    }

    pub fn with_token<S: Into<String>>(mut self, token: S) -> Self {
        self.token = Some(token.into());
        self
    }

    pub fn with_node_bin<P: AsRef<Path>>(mut self, node_bin: P) -> Self {
        self.node_bin = Some(node_bin.as_ref().to_path_buf());
        self
    }

    pub fn with_server_script<P: AsRef<Path>>(mut self, script: P) -> Self {
        self.server_script = Some(script.as_ref().to_path_buf());
        self
    }

    pub fn with_external_url<S: Into<String>>(mut self, url: S) -> Self {
        self.external_url = Some(url.into());
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

fn clean_path(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let s = path.to_string_lossy().to_string();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            return PathBuf::from(stripped);
        }
    }
    path
}

/// Resolves the absolute path to `src/server/index.ts`.
pub fn find_server_script(explicit: Option<&Path>, project_root: &Path) -> Result<PathBuf> {
    if let Some(path) = explicit {
        if path.exists() {
            let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            return Ok(clean_path(canon));
        }
    }

    let candidates = [
        project_root.join("src").join("server").join("index.ts"),
        PathBuf::from("src").join("server").join("index.ts"),
        PathBuf::from("..").join("src").join("server").join("index.ts"),
        PathBuf::from("..").join("..").join("src").join("server").join("index.ts"),
    ];

    for candidate in &candidates {
        if candidate.exists() {
            let canon = candidate.canonicalize().unwrap_or_else(|_| candidate.clone());
            return Ok(clean_path(canon));
        }
    }

    // Try current_exe dir walk up
    if let Ok(mut exe) = std::env::current_exe() {
        for _ in 0..5 {
            if exe.pop() {
                let candidate = exe.join("src").join("server").join("index.ts");
                if candidate.exists() {
                    let canon = candidate.canonicalize().unwrap_or_else(|_| candidate.clone());
                    return Ok(clean_path(canon));
                }
            }
        }
    }

    Err(NexusError::SidecarStartFailed(format!(
        "Could not find src/server/index.ts from project root {}",
        project_root.display()
    )))
}

/// Manages the running sidecar process and its connection.
pub struct SidecarManager {
    child: Option<Child>,
    ready_info: SidecarReadyInfo,
    base_url: String,
    token: Option<String>,
}

impl SidecarManager {
    /// Start sidecar or connect to an external server according to configuration.
    pub fn start(config: SidecarConfig) -> Result<Self> {
        if let Some(external_url) = config.external_url {
            let base_url = external_url.trim_end_matches('/').to_string();
            let ready_info = SidecarReadyInfo {
                protocol: SIDECAR_READY_PROTOCOL.into(),
                protocol_version: 1,
                pid: 0,
                host: "external".into(),
                port: 0,
                started_at: "external".into(),
                base_url: Some(base_url.clone()),
                project_root: Some(config.project_root.to_string_lossy().to_string()),
                version: None,
                token_required: Some(config.token.is_some()),
            };

            return Ok(Self {
                child: None,
                ready_info,
                base_url,
                token: config.token,
            });
        }

        let server_script = find_server_script(config.server_script.as_deref(), &config.project_root)?;
        let node_bin = config.node_bin.unwrap_or_else(|| PathBuf::from("node"));

        let mut cmd = Command::new(&node_bin);
        cmd.arg("--no-warnings");
        cmd.arg(&server_script);
        cmd.arg("--port");
        cmd.arg(config.port.to_string());
        cmd.arg("--host");
        cmd.arg(&config.host);
        cmd.arg("--project-root");
        let clean_root = clean_path(config.project_root.clone());
        cmd.arg(&clean_root);

        if let Some(ref home) = config.home {
            cmd.arg("--home");
            cmd.arg(home);
        }

        if let Some(ref token) = config.token {
            cmd.arg("--token");
            cmd.arg(token);
        }

        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);

        let mut child = cmd.spawn().map_err(|e| {
            NexusError::SidecarStartFailed(format!(
                "Failed to spawn node sidecar {:?}: {e}",
                node_bin
            ))
        })?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| NexusError::SidecarStartFailed("Failed to capture sidecar stdout".into()))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| NexusError::SidecarStartFailed("Failed to capture sidecar stderr".into()))?;

        // Background thread to read stdout lines looking for SidecarReadyInfo
        let (tx, rx) = mpsc::channel();
        let reader_thread = thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line_res in reader.lines() {
                match line_res {
                    Ok(line) => {
                        let trimmed = line.trim();
                        if trimmed.starts_with('{') && trimmed.contains(SIDECAR_READY_PROTOCOL) {
                            if let Ok(info) = serde_json::from_str::<SidecarReadyInfo>(trimmed) {
                                let _ = tx.send(Ok(info));
                                return;
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        return;
                    }
                }
            }
        });

        // Wait for ready info with timeout
        let ready_res = match rx.recv_timeout(config.timeout) {
            Ok(Ok(info)) => Ok(info),
            Ok(Err(e)) => Err(NexusError::SidecarStartFailed(format!("Error reading sidecar stdout: {e}"))),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = child.kill();
                let _ = child.wait();
                let mut err_buf = String::new();
                let _ = stderr.read_to_string(&mut err_buf);
                Err(NexusError::SidecarTimeout(format!(
                    "Timed out waiting for sidecar ready info ({}s). Stderr: {}",
                    config.timeout.as_secs(),
                    err_buf.trim()
                )))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let status = child.wait().ok();
                let mut err_buf = String::new();
                let _ = stderr.read_to_string(&mut err_buf);
                Err(NexusError::SidecarExited {
                    code: status.and_then(|s| s.code()),
                    stderr: err_buf,
                })
            }
        };

        // Don't leak reader thread
        let _ = reader_thread;

        let ready_info = ready_res?;
        let base_url = ready_info
            .base_url
            .clone()
            .unwrap_or_else(|| format!("http://{}:{}", ready_info.host, ready_info.port));

        Ok(Self {
            child: Some(child),
            ready_info,
            base_url,
            token: config.token,
        })
    }

    /// Base URL of the sidecar HTTP listener.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Ready handshake info received from the sidecar.
    pub fn ready_info(&self) -> &SidecarReadyInfo {
        &self.ready_info
    }

    /// Configured Bearer token, if any.
    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    /// Create a strongly-typed `NexusClient` bound to this sidecar.
    pub fn create_client(&self) -> NexusClient {
        NexusClient::new(&self.base_url, self.token.clone())
    }

    /// Clean shutdown: closes stdin first to let sidecar exit gracefully,
    /// falls back to kill if not terminated within 2 seconds.
    pub fn shutdown(&mut self) -> Result<()> {
        if let Some(mut child) = self.child.take() {
            // Close stdin to trigger EOF handler in src/server/index.ts
            drop(child.stdin.take());

            let start = Instant::now();
            let grace = Duration::from_millis(1500);

            while start.elapsed() < grace {
                if let Ok(Some(_)) = child.try_wait() {
                    return Ok(());
                }
                thread::sleep(Duration::from_millis(50));
            }

            // Force kill if still running
            let _ = child.kill();
            let _ = child.wait();
        }
        Ok(())
    }
}

impl Drop for SidecarManager {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
