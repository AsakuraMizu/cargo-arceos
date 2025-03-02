mod commands;
mod options;
mod platforms;

use std::{io::BufReader, process::ExitStatus};

use clap::Parser;
use console::style;

#[derive(Debug, Parser)]
#[command(
    version,
    about,
    styles = cargo_options::styles(),
    // Cargo passes in the subcommand name to the invoked executable. Use a
    // hidden, optional positional argument to deal with it.
    arg(clap::Arg::new("dummy")
        .value_parser([clap::builder::PossibleValue::new("arceos")])
        .required(false)
        .hide(true))
)]
pub enum Cli {
    #[command(alias = "b")]
    Build(commands::Build),
    Rustc(commands::Rustc),
    Check(commands::Check),
    Clippy(commands::Clippy),
    #[command(alias = "r")]
    Run(commands::Run),
    #[command(hide = true)]
    Runner(commands::Runner),
}

impl Cli {
    pub fn execute(self) {
        let (mut command, arceos) = match self {
            Cli::Build(mut command) => (command.build(), command.arceos),
            Cli::Rustc(mut command) => (command.build(), command.arceos),
            Cli::Check(mut command) => (command.build(), command.arceos),
            Cli::Clippy(mut command) => (command.build(), command.arceos),
            Cli::Run(mut command) => (command.build(), command.arceos),
            Cli::Runner(command) => {
                command.execute();
                return;
            }
        };

        let mut child = command.spawn().expect("failed to execute cargo");

        if let Some(stdout) = child.stdout.take().map(BufReader::new) {
            for message in cargo_metadata::Message::parse_stream(stdout).flatten() {
                match message {
                    cargo_metadata::Message::TextLine(line) => {
                        println!("{}", line);
                    }
                    cargo_metadata::Message::CompilerArtifact(artifact) => {
                        arceos.check_features(&artifact.target.name, &artifact.features);
                    }
                    _ => {}
                }
            }
        }

        let status = child.wait().expect("could not get cargo's exit status");
        std::process::exit(status.code().unwrap_or(101));
    }
}

fn check_exit_status(program: &str, status: ExitStatus) {
    if !status.success() {
        eprintln!(
            "{}: {} {}",
            style("error").red().bold().for_stderr(),
            program,
            status
        );
        std::process::exit(status.code().unwrap_or(101));
    }
}
