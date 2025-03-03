use std::{
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::Context;
use axconfig_gen::{Config, ConfigValue};
use clap::{Args, ValueEnum};
use heck::ToShoutySnakeCase;

use crate::platforms::{Arch, Platform};

struct Feature {
    name: &'static str,
    cond: &'static str,
    packages: &'static [&'static str],
}

const SMP: Feature = Feature {
    name: "smp",
    cond: "number of CPUs > 1",
    packages: &[
        "axlibc",
        "arceos_posix_api",
        "axstd",
        "axfeat",
        "axhal",
        "axruntime",
        "axtask",
    ],
};

const FP_SIMD: Feature = Feature {
    name: "fp_simd",
    cond: "compiling to AArch64 without soft float",
    packages: &["axlibc", "axstd", "axfeat", "axhal"],
};

#[derive(Debug, Clone, Args)]
#[command(next_help_heading = "ArceOS Options")]
pub struct ArceOSOptions {
    #[command(flatten)]
    arch_or_platform: ArchOrPlatform,

    /// Enable soft float
    #[arg(long, env)]
    soft_float: bool,

    /// Number of CPUs
    #[arg(long, default_value_t = 1, env, value_name = "N")]
    cpus: u32,

    /// Additional config files
    #[arg(short, long, env, value_name = "PATH")]
    configs: Vec<PathBuf>,

    /// Log level
    #[arg(short = 'L', long, default_value_t = log::LevelFilter::Warn, env, value_name = "LEVEL")]
    log: log::LevelFilter,
}

#[derive(Debug, Clone, Args)]
#[group(multiple = false)]
struct ArchOrPlatform {
    /// Target architecture
    #[arg(short = 'A', long, env)]
    arch: Option<Arch>,

    /// Target platform
    #[arg(short = 'P', long, env)]
    platform: Option<Platform>,
}

impl From<ArchOrPlatform> for Platform {
    fn from(value: ArchOrPlatform) -> Self {
        if let Some(arch) = value.arch {
            arch.into()
        } else if let Some(platform) = value.platform {
            platform
        } else {
            Platform::Dummy
        }
    }
}

impl ArceOSOptions {
    #[inline]
    pub fn platform(&self) -> Platform {
        self.arch_or_platform.clone().into()
    }

    #[inline]
    pub fn arch(&self) -> Arch {
        self.platform().into()
    }

    #[inline]
    pub fn target(&self) -> &'static str {
        match (self.arch(), self.soft_float) {
            (Arch::Aarch64, false) => "aarch64-unknown-none",
            (Arch::Aarch64, true) => "aarch64-unknown-none-softfloat",
            (Arch::Loongarch64, _) => "loongarch64-unknown-none",
            (Arch::Riscv64, _) => "riscv64gc-unknown-none-elf",
            (Arch::X86_64, _) => "x86_64-unknown-none",
        }
    }

    pub fn apply(
        &self,
        target_dir: &Path,
        profile: &str,
        command: &mut Command,
    ) -> anyhow::Result<()> {
        let platform: Platform = self.platform();
        let arch: Arch = self.arch();
        let target = self.target();

        command.args(["--target", target]);

        let binary_dir = target_dir.join(target).join(profile);

        // Update config file
        let config_path = binary_dir.join("axconfig.toml");
        if !config_path.exists() {
            std::fs::create_dir_all(&binary_dir).context("failed to create target directory")?;
        }

        let mut config: Config = platform.into();
        for path in &self.configs {
            let toml = std::fs::read_to_string(path)
                .with_context(|| format!("failed to read config file `{}`", path.display()))?;
            let c = Config::from_toml(&toml).map_err(|e| {
                anyhow::anyhow!("failed to parse config file `{}`: {}", path.display(), e)
            })?;
            config.merge(&c).map_err(|e| {
                anyhow::anyhow!("failed to merge config file `{}`: {}", path.display(), e)
            })?;
        }
        config
            .config_at_mut(Config::GLOBAL_TABLE_NAME, "smp")
            .unwrap()
            .value_mut()
            .update(ConfigValue::new(&self.cpus.to_string()).unwrap())
            .unwrap();
        let config = config.dump_toml().unwrap();

        if std::fs::read_to_string(&config_path)
            .ok()
            .is_none_or(|old_config| old_config != config)
        {
            std::fs::write(&config_path, config).context("failed to write config file")?;
        }

        // Set environment variables
        command.env("AX_CONFIG_PATH", config_path.canonicalize().unwrap());
        command.env("AX_PLATFORM", platform.to_string());
        command.env("AX_ARCH", arch.to_string());
        command.env("AX_SMP", self.cpus.to_string());
        command.env("AX_TARGET", target);
        command.env("AX_MODE", profile);
        command.env("AX_LOG", self.log.to_string());

        if !matches!(platform, Platform::Dummy) {
            // Set link flags
            command.env(
                "RUSTFLAGS",
                format!(
                    "-C link-arg=-T{}/linker_{}.lds -C link-arg=-no-pie -C link-arg=-znostart-stop-gc",
                    binary_dir.display(),
                    platform
                ),
            );
        }

        Ok(())
    }

    fn features(&self) -> Vec<&Feature> {
        let mut features = vec![];

        if self.cpus > 1 {
            features.push(&SMP);
        }

        if matches!(self.arch(), Arch::Aarch64) && !self.soft_float {
            features.push(&FP_SIMD);
        }

        features
    }

    pub fn check_features(&self, package: &str, features: &[String]) {
        for f in self.features() {
            if f.packages.contains(&package) && !features.contains(&f.name.to_string()) {
                crate::warn(format!(
                    "feature `{}` should be enabled for package `{}` when {}",
                    f.name, package, f.cond
                ));
            }
        }
    }
}

#[derive(Debug, Clone, Args)]
#[command(next_help_heading = "QEMU Options")]
pub struct QEMUOptions {
    /// Simulate a SMP system
    #[arg(long, env)]
    smp: Option<String>,
    /// RAM size
    #[arg(short, long, env)]
    mem: Option<String>,
}

impl QEMUOptions {
    pub fn apply(&self, target: &str, command: &mut Command) {
        let mut runner = vec!["cargo-arceos", "runner"];

        if let Some(smp) = &self.smp {
            runner.push("--smp");
            runner.push(smp);
        }
        if let Some(mem) = &self.mem {
            runner.push("--mem");
            runner.push(mem);
        }

        command.env(
            format!("CARGO_TARGET_{}_RUNNER", target.to_shouty_snake_case()),
            runner.join(" "),
        );
    }

    pub fn execute(&self, binary: &Path) -> anyhow::Result<()> {
        let platform = Platform::from_str(&std::env::var("AX_PLATFORM")?, false).unwrap();

        let (machine, mem) = match platform {
            Platform::AARCH64_QEMU_VIRT => ("virt", None),
            Platform::AARCH64_RASPI4 => ("raspi4b", Some("2G")),
            Platform::LOONGARCH64_QEMU_VIRT => ("virt", Some("1G")),
            Platform::RISCV64_QEMU_VIRT => ("virt", None),
            Platform::X86_64_QEMU_Q35 => ("q35", None),
            _ => anyhow::bail!("unsupported platform: {}", platform),
        };

        let arch: Arch = platform.into();

        let program = match arch {
            Arch::Aarch64 => "qemu-system-aarch64",
            Arch::Loongarch64 => "qemu-system-loongarch64",
            Arch::Riscv64 => "qemu-system-riscv64",
            Arch::X86_64 => "qemu-system-x86_64",
        };
        let kernel = match arch {
            Arch::Aarch64 | Arch::Riscv64 => {
                let elf = binary;
                let kernel = binary.with_extension("bin");

                let mut command = Command::new("rust-objcopy");
                command
                    .args(["--strip-all", "-O", "binary"])
                    .arg(elf)
                    .arg(&kernel);
                crate::run_command(&mut command)?;

                kernel
            }
            _ => binary.to_path_buf(),
        };

        let mut command = Command::new(program);

        command
            .arg("-kernel")
            .arg(kernel)
            .args(["-machine", machine]);

        if let Arch::Aarch64 = arch {
            command.args(["-cpu", "cortex-a72"]);
        }

        if let Some(mem) = self.mem.as_deref().or(mem) {
            command.args(["-m", mem]);
        }

        let cpus = std::env::var("AX_SMP").unwrap();
        command.args(["-smp", self.smp.as_deref().unwrap_or(&cpus)]);

        command.arg("-nographic");

        crate::run_command(&mut command)
    }
}
