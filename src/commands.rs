use std::{
    path::PathBuf,
    process::{Command, Stdio},
};

use anyhow::Context;
use clap::Args;

trait CargoOptionsExt {
    fn build(&mut self) -> Command;
    fn target_dir(&self) -> anyhow::Result<PathBuf>;
    fn profile(&self) -> &str;
}

macro_rules! impl_cargo_options_ext {
    ($command:path) => {
        impl CargoOptionsExt for $command {
            fn build(&mut self) -> Command {
                if !self.target.is_empty() {
                    self.target.clear();
                    crate::warn("`--target` option is ignored");
                }

                let mut command = self.command();
                if self.message_format.is_empty() {
                    command
                        .arg("--message-format=json-render-diagnostics")
                        .stdout(Stdio::piped());
                }
                command
            }

            fn target_dir(&self) -> anyhow::Result<PathBuf> {
                if let Some(target_dir) = &self.target_dir {
                    return Ok(PathBuf::from(target_dir));
                }

                let mut metadata = cargo_metadata::MetadataCommand::new();
                if let Some(manifest_path) = &self.manifest_path {
                    metadata.manifest_path(manifest_path);
                }
                metadata.no_deps();
                let metadata = metadata.exec().context("failed to get metadata")?;

                Ok(metadata.target_directory.into())
            }

            fn profile(&self) -> &str {
                if self.release {
                    "release"
                } else if let Some(profile) = &self.profile {
                    profile
                } else {
                    "debug"
                }
            }
        }
    };
}

impl_cargo_options_ext!(cargo_options::Build);
impl_cargo_options_ext!(cargo_options::Rustc);
impl_cargo_options_ext!(cargo_options::Check);
impl_cargo_options_ext!(cargo_options::Clippy);
impl_cargo_options_ext!(cargo_options::Run);
impl_cargo_options_ext!(cargo_options::Test);

macro_rules! command {
    ($command:ident) => {
        #[derive(Debug, Args)]
        pub struct $command {
            #[command(flatten)]
            cargo: cargo_options::$command,
            #[command(flatten)]
            pub arceos: crate::options::ArceOSOptions,
        }

        impl $command {
            pub fn build(&mut self) -> anyhow::Result<Command> {
                let mut command = self.cargo.build();

                let target_dir = self.cargo.target_dir()?;
                let profile = self.cargo.profile();
                self.arceos.apply(&target_dir, profile, &mut command)?;

                Ok(command)
            }
        }
    };
}

command!(Build);
command!(Rustc);
command!(Check);
command!(Clippy);

#[derive(Debug, Args)]
pub struct Run {
    #[command(flatten)]
    cargo: cargo_options::Run,
    #[command(flatten)]
    pub arceos: crate::options::ArceOSOptions,
    #[command(flatten)]
    qemu: crate::options::QEMUOptions,
}

impl Run {
    pub fn build(&mut self) -> anyhow::Result<Command> {
        let mut command = self.cargo.build();

        let target_dir = self.cargo.target_dir()?;
        let profile = self.cargo.profile();
        self.arceos.apply(&target_dir, profile, &mut command)?;

        self.qemu.apply(self.arceos.target(), &mut command);

        Ok(command)
    }
}

#[derive(Debug, Args)]
pub struct Runner {
    #[command(flatten)]
    qemu: crate::options::QEMUOptions,
    binary: PathBuf,
}

impl Runner {
    pub fn execute(&self) -> anyhow::Result<()> {
        self.qemu.execute(&self.binary)
    }
}
