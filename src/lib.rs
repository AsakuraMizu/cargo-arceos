mod commands;
mod options;
mod platforms;

use std::io::BufReader;

use anyhow::bail;
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
    pub fn run(self) {
        if let Err(e) = self.execute() {
            eprintln!("{}: {}", style("error").for_stderr().red().bold(), e);
        }
    }

    fn execute(self) -> anyhow::Result<()> {
        let (mut command, arceos) = match self {
            Cli::Build(mut command) => (command.build()?, command.arceos),
            Cli::Rustc(mut command) => (command.build()?, command.arceos),
            Cli::Check(mut command) => (command.build()?, command.arceos),
            Cli::Clippy(mut command) => (command.build()?, command.arceos),
            Cli::Run(mut command) => (command.build()?, command.arceos),
            Cli::Runner(command) => {
                return command.execute();
            }
        };

        let mut child = command.spawn().expect("failed to execute cargo");

        if let Some(stdout) = child.stdout.take().map(BufReader::new) {
            for message in cargo_metadata::Message::parse_stream(stdout).flatten() {
                match message {
                    cargo_metadata::Message::TextLine(line) => {
                        eprintln!("{}", line);
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

fn info(name: &str, msg: impl std::fmt::Display) {
    eprintln!("{:>12} {}", style(name).for_stderr().green().bold(), msg);
}

fn warn(msg: impl std::fmt::Display) {
    eprintln!(
        "{}",
        style(format!(
            "{}: {}",
            style("warning").for_stderr().yellow(),
            msg
        ))
        .for_stderr()
        .bold()
    );
}

fn run_command(command: &mut std::process::Command) -> anyhow::Result<()> {
    info(
        "Running",
        format!(
            "`{} {}`",
            command.get_program().to_string_lossy(),
            command
                .get_args()
                .map(|arg| arg.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ")
        ),
    );

    let status = command.status()?;
    if !status.success() {
        bail!("command failed with {}", status);
    }
    Ok(())
}
