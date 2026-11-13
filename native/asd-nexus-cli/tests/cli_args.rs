use asd_nexus_cli::{Cli, Command};
use clap::Parser;

#[test]
fn defaults_launch_tui() {
    let cli = Cli::try_parse_from(["asd-nexus"]).unwrap();
    assert_eq!(cli.command, None);
    assert_eq!(cli.harness, "claude-code");
    assert!(cli.server_url.is_none());
    assert!(!cli.json);
}

#[test]
fn global_flags_and_subcommands() {
    let cli = Cli::try_parse_from([
        "asd-nexus",
        "--project-root",
        "C:/repo",
        "--server-url",
        "http://127.0.0.1:4555",
        "--token",
        "secret",
        "--harness",
        "codex",
        "--model",
        "gpt-5",
        "--session",
        "asd-1",
        "--json",
        "board",
    ])
    .unwrap();
    assert_eq!(cli.command, Some(Command::Board));
    assert_eq!(cli.project_root_string(), "C:/repo");
    assert_eq!(cli.server_url.as_deref(), Some("http://127.0.0.1:4555"));
    assert_eq!(cli.token.as_deref(), Some("secret"));
    assert_eq!(cli.harness, "codex");
    assert_eq!(cli.model.as_deref(), Some("gpt-5"));
    assert_eq!(cli.session.as_deref(), Some("asd-1"));
    assert!(cli.json);
}

#[test]
fn global_flags_after_subcommand() {
    let cli = Cli::try_parse_from(["asd-nexus", "sessions", "--limit", "5", "--json", "--harness", "cursor"]).unwrap();
    assert_eq!(cli.command, Some(Command::Sessions { limit: Some(5) }));
    assert!(cli.json);
    assert_eq!(cli.harness, "cursor");
    let cli = Cli::try_parse_from(["asd-nexus", "health"]).unwrap();
    assert_eq!(cli.command, Some(Command::Health));
}

#[test]
fn all_five_harnesses_accepted_and_unknown_rejected() {
    for h in ["claude-code", "codex", "opencode", "antigravity", "cursor"] {
        assert_eq!(Cli::try_parse_from(["asd-nexus", "--harness", h]).unwrap().harness, h);
    }
    assert!(Cli::try_parse_from(["asd-nexus", "--harness", "gemini"]).is_err());
    assert!(Cli::try_parse_from(["asd-nexus", "bogus"]).is_err());
}
