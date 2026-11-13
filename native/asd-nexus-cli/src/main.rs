use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    // Return ExitCode (not process::exit) so the sidecar and terminal guards Drop cleanly.
    match asd_nexus_cli::run(asd_nexus_cli::Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}
