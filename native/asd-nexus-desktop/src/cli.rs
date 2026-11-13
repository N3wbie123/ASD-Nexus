//! Command-line flags for the desktop app (no external parser dependency).

use std::path::PathBuf;

pub const USAGE: &str = "\
asd-nexus-desktop — pure native ASD-Nexus desktop GUI (eframe/egui, 0% WebView)

USAGE:
    asd-nexus-desktop [OPTIONS]

OPTIONS:
    --project-root <dir>     Project root (default: current directory)
    --server-url <url>       Connect to an existing sidecar instead of spawning one
    --token <tok>            Bearer token for the sidecar
    --home <dir>             Override ASD home directory passed to the spawned sidecar
    --node <path>            Node binary used to spawn the sidecar (default: node)
    --server-script <path>   Explicit path to src/server/index.ts
    --verify                 Non-interactive: connect, render every tab headlessly,
                             print a JSON report and exit (no window)
    --exit-after <secs>      Open the window and close it automatically after <secs>
    --screenshot <file.bmp>  Open the window, save a screenshot (24-bit BMP) and exit
    -h, --help               Print this help
";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CliArgs {
    pub project_root: Option<PathBuf>,
    pub server_url: Option<String>,
    pub token: Option<String>,
    pub home: Option<PathBuf>,
    pub node_bin: Option<PathBuf>,
    pub server_script: Option<PathBuf>,
    pub verify: bool,
    pub exit_after_secs: Option<f64>,
    pub screenshot: Option<PathBuf>,
    pub help: bool,
}

impl CliArgs {
    /// Parse arguments (excluding argv[0]).
    pub fn parse<I, S>(args: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut out = CliArgs::default();
        let mut it = args.into_iter().map(Into::into);
        while let Some(raw) = it.next() {
            let (flag, inline) = match raw.split_once('=') {
                Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
                _ => (raw.clone(), None),
            };
            let mut value = |name: &str| -> Result<String, String> {
                match inline.clone().or_else(|| it.next()) {
                    Some(v) if !v.is_empty() => Ok(v),
                    _ => Err(format!("missing value for {name}")),
                }
            };
            match flag.as_str() {
                "--project-root" => out.project_root = Some(PathBuf::from(value("--project-root")?)),
                "--server-url" => out.server_url = Some(value("--server-url")?),
                "--token" => out.token = Some(value("--token")?),
                "--home" => out.home = Some(PathBuf::from(value("--home")?)),
                "--node" => out.node_bin = Some(PathBuf::from(value("--node")?)),
                "--server-script" => out.server_script = Some(PathBuf::from(value("--server-script")?)),
                "--exit-after" => {
                    let v = value("--exit-after")?;
                    let secs: f64 = v.parse().map_err(|_| format!("invalid --exit-after value: {v}"))?;
                    if !(secs >= 0.0) {
                        return Err(format!("invalid --exit-after value: {v}"));
                    }
                    out.exit_after_secs = Some(secs);
                }
                "--screenshot" => out.screenshot = Some(PathBuf::from(value("--screenshot")?)),
                "--verify" => out.verify = true,
                "-h" | "--help" => out.help = true,
                other => return Err(format!("unknown argument: {other}\n\n{USAGE}")),
            }
        }
        Ok(out)
    }

    /// Effective project root (flag or current directory), made absolute.
    pub fn effective_project_root(&self) -> PathBuf {
        let p = self
            .project_root
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        match p.canonicalize() {
            Ok(c) => {
                let s = c.to_string_lossy().to_string();
                PathBuf::from(s.strip_prefix(r"\\?\").unwrap_or(&s))
            }
            Err(_) => p,
        }
    }

    /// Whether a sidecar must be spawned (no external server URL given).
    pub fn spawns_sidecar(&self) -> bool {
        self.server_url.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_flags() {
        let a = CliArgs::parse([
            "--project-root",
            "C:/p",
            "--server-url=http://127.0.0.1:9",
            "--token",
            "t",
            "--verify",
            "--exit-after",
            "2.5",
            "--screenshot",
            "s.bmp",
        ])
        .unwrap();
        assert_eq!(a.project_root, Some(PathBuf::from("C:/p")));
        assert_eq!(a.server_url.as_deref(), Some("http://127.0.0.1:9"));
        assert_eq!(a.token.as_deref(), Some("t"));
        assert!(a.verify);
        assert_eq!(a.exit_after_secs, Some(2.5));
        assert_eq!(a.screenshot, Some(PathBuf::from("s.bmp")));
        assert!(!a.spawns_sidecar());
    }

    #[test]
    fn defaults_spawn_sidecar() {
        let a = CliArgs::parse(Vec::<String>::new()).unwrap();
        assert!(a.spawns_sidecar());
        assert!(!a.verify);
    }

    #[test]
    fn errors() {
        assert!(CliArgs::parse(["--token"]).is_err());
        assert!(CliArgs::parse(["--bogus"]).is_err());
        assert!(CliArgs::parse(["--exit-after", "x"]).is_err());
        assert!(CliArgs::parse(["--exit-after", "-1"]).is_err());
        assert!(CliArgs::parse(["-h"]).unwrap().help);
    }
}
