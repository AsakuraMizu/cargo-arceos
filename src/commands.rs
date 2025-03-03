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
    (@common) => {
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
    };
    (@args $self:ident) => {
        if !$self.args.is_empty() {
            crate::warn(format!("extra args `{}` is ignored", $self.args.join(" ")));
            $self.args.clear();
        }
    };
    (@target $self:ident) => {
        if !$self.target.is_empty() {
            $self.target.clear();
            crate::warn("`--target` option is ignored");
        }
    };
    (@stdout $self:ident $command:ident) => {
        if $self.message_format.is_empty() {
            $command
                .arg("--message-format=json-render-diagnostics")
                .stdout(Stdio::piped());
        }
    };
    ($command:path) => {
        impl CargoOptionsExt for $command {
            fn build(&mut self) -> Command {
                impl_cargo_options_ext!(@args self);
                impl_cargo_options_ext!(@target self);
                let mut command = self.command();
                impl_cargo_options_ext!(@stdout self command);
                command
            }
            impl_cargo_options_ext!(@common);
        }
    };
    (no_arg $command:path) => {
        impl CargoOptionsExt for $command {
            fn build(&mut self) -> Command {
                impl_cargo_options_ext!(@target self);
                let mut command = self.command();
                impl_cargo_options_ext!(@stdout self command);
                command
            }
            impl_cargo_options_ext!(@common);
        }
    };
    (no_stdout $command:path) => {
        impl CargoOptionsExt for $command {
            fn build(&mut self) -> Command {
                impl_cargo_options_ext!(@args self);
                impl_cargo_options_ext!(@target self);
                self.command()
            }
            impl_cargo_options_ext!(@common);
        }
    }
}

impl_cargo_options_ext!(no_arg cargo_options::Build);
impl_cargo_options_ext!(cargo_options::Rustc);
impl_cargo_options_ext!(no_arg cargo_options::Check);
impl_cargo_options_ext!(cargo_options::Clippy);
impl_cargo_options_ext!(no_stdout cargo_options::Run);
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
    pub fn execute(self) -> anyhow::Result<()> {
        self.qemu.execute(self.binary)
    }
}
