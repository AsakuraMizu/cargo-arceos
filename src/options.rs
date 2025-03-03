use std::{
    env, fs,
    net::Ipv4Addr,
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
};

use anyhow::Context;
use axconfig_gen::{Config, ConfigValue};
use clap::{Args, builder::TypedValueParser};
use heck::ToShoutySnakeCase;
use strum::{AsRefStr, EnumString, VariantNames};

use crate::platforms::{Arch, Platform};

// https://github.com/clap-rs/clap/discussions/4264
macro_rules! enum_variants {
    ($e:ty) => {
        clap::builder::PossibleValuesParser::new(<$e>::VARIANTS).map(|s| s.parse::<$e>().unwrap())
    };
}

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

    /// IP address
    #[arg(long, env, default_value = "10.0.2.15", value_name = "ADDR")]
    ip: Ipv4Addr,

    /// Gateway
    #[arg(long, env = "GW", default_value = "10.0.2.2", value_name = "ADDR")]
    gateway: Ipv4Addr,
}

#[derive(Debug, Clone, Args)]
#[group(multiple = false)]
struct ArchOrPlatform {
    /// Target architecture
    #[arg(short = 'A', long, env, value_parser = enum_variants!(Arch))]
    arch: Option<Arch>,

    /// Target platform
    #[arg(short = 'P', long, env, value_parser = enum_variants!(Platform))]
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
            fs::create_dir_all(&binary_dir).context("failed to create target directory")?;
        }

        let mut config: Config = platform.into();
        for path in &self.configs {
            let toml = fs::read_to_string(path)
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

        if fs::read_to_string(&config_path)
            .ok()
            .is_none_or(|old_config| old_config != config)
        {
            fs::write(&config_path, config).context("failed to write config file")?;
        }

        // Set environment variables
        command.env("AX_CONFIG_PATH", config_path.canonicalize().unwrap());
        command.env("AX_PLATFORM", platform.as_ref());
        command.env("AX_ARCH", arch.as_ref());
        command.env("AX_SMP", self.cpus.to_string());
        command.env("AX_TARGET", target);
        command.env("AX_MODE", profile);
        command.env("AX_LOG", self.log.to_string());
        command.env("AX_IP", self.ip.to_string());
        command.env("AX_GW", self.gateway.to_string());

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
    #[arg(short, long)]
    mem: Option<String>,

    /// Device bus type
    #[arg(long, value_parser = enum_variants!(BusType))]
    bus: Option<BusType>,

    /// Enable network device and possibly specify the type
    #[arg(long, require_equals = true, value_parser = enum_variants!(NetDevType))]
    net: Option<Option<NetDevType>>,

    /// Dump network packets to a file
    #[arg(long, requires = "net", value_name = "FILE")]
    net_dump: Option<PathBuf>,

    /// Disk image
    #[arg(short, long)]
    disk: Option<PathBuf>,

    /// Enable graphics
    #[arg(short, long)]
    graphics: bool,

    /// Enable hardware acceleration (KVM on Linux or HVF on macOS)
    #[arg(long)]
    accel: bool,

    /// Enable debugging
    #[arg(short = 'D', long, conflicts_with = "accel")]
    debug: bool,
}

#[derive(Debug, Default, Clone, EnumString, VariantNames, AsRefStr)]
#[strum(serialize_all = "snake_case")]
pub enum BusType {
    #[default]
    Pci,
    Mmio,
}

impl BusType {
    fn vdev_suffix(&self) -> &'static str {
        match self {
            BusType::Pci => "pci",
            BusType::Mmio => "device",
        }
    }
}

#[derive(Debug, Default, Clone, EnumString, VariantNames, AsRefStr)]
#[strum(serialize_all = "snake_case")]
pub enum NetDevType {
    #[default]
    User,
    // TODO: tap / bridge
}

impl QEMUOptions {
    pub fn apply(&self, target: &str, command: &mut Command) {
        let mut runner: String = "cargo-arceos runner".to_string();

        if let Some(smp) = &self.smp {
            runner.push_str(" --smp ");
            runner.push_str(smp);
        }

        if let Some(mem) = &self.mem {
            runner.push_str(" --mem ");
            runner.push_str(mem);
        }

        if let Some(bus) = &self.bus {
            runner.push_str(" --bus ");
            runner.push_str(bus.as_ref());
        }

        if let Some(net) = &self.net {
            runner.push_str(" --net");
            if let Some(net) = net {
                runner.push('=');
                runner.push_str(net.as_ref());
            }
        }

        if let Some(dump) = &self.net_dump {
            runner.push_str(" --net-dump ");
            runner.push_str(dump.to_str().unwrap());
        }

        if let Some(disk) = &self.disk {
            runner.push_str(" --disk ");
            runner.push_str(disk.to_str().unwrap());
        }

        if self.graphics {
            runner.push_str(" --graphics");
        }

        if self.accel {
            runner.push_str(" --accel");
        }

        if self.debug {
            runner.push_str(" --debug");
        }

        command.env(
            format!("CARGO_TARGET_{}_RUNNER", target.to_shouty_snake_case()),
            runner,
        );
    }

    pub fn execute(self, binary: PathBuf) -> anyhow::Result<()> {
        let platform = Platform::from_str(&env::var("AX_PLATFORM")?)?;

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
                let kernel = binary.with_extension("bin");

                let mut command = Command::new("rust-objcopy");
                command
                    .args(["--strip-all", "-O", "binary"])
                    .arg(binary)
                    .arg(&kernel);
                crate::run_command(&mut command)?;

                kernel
            }
            _ => binary,
        };

        let mut command = Command::new(program);

        let cpus = env::var("AX_SMP").unwrap();
        command
            .arg("-kernel")
            .arg(kernel)
            .args(["-machine", machine])
            .args(["-smp", self.smp.as_deref().unwrap_or(&cpus)]);

        if let Arch::Aarch64 = arch {
            command.args(["-cpu", "cortex-a72"]);
        }

        if let Some(mem) = self.mem.as_deref().or(mem) {
            command.args(["-m", mem]);
        }

        let vdev_suffix = self.bus.unwrap_or_default().vdev_suffix();

        if let Some(net) = self.net {
            command
                .arg("-device")
                .arg(format!("virtio-net-{},netdev=net0", vdev_suffix))
                .arg("-netdev");
            match net.unwrap_or_default() {
                NetDevType::User => {
                    command.arg("user,id=net0,hostfwd=tcp::5555-:5555,hostfwd=udp::5555-:5555")
                }
            };
        }

        if let Some(dump) = self.net_dump {
            command.arg("-object").arg(format!(
                "filter-dump,id=dump0,netdev=net0,file={}",
                dump.display()
            ));
        }

        if let Some(disk) = self.disk {
            command
                .arg("-device")
                .arg(format!("virtio-blk-{},drive=disk0", vdev_suffix))
                .arg("-drive")
                .arg(format!(
                    "id=disk0,if=none,format=raw,file={}",
                    disk.display()
                ));
        }

        if self.graphics {
            command
                .arg("-device")
                .arg(format!("virtio-gpu-{}", vdev_suffix))
                .args(["-vga", "none", "-serial", "mon:stdio"]);
        } else {
            command.arg("-nographic");
        }

        if self.debug {
            command.args(["-s", "-S"]);
        } else {
            let accel = self.accel || {
                (if cfg!(target_arch = "x86_64") {
                    matches!(arch, Arch::X86_64)
                } else if cfg!(target_arch = "aarch64") {
                    matches!(arch, Arch::Aarch64)
                } else {
                    false
                }) && {
                    if cfg!(target_vendor = "apple") {
                        true
                    } else {
                        Path::new("/dev/kvm").exists()
                    }
                }
            };
            if accel {
                command.args([
                    "-cpu",
                    "host",
                    "-accel",
                    if cfg!(target_vendor = "apple") {
                        "hvf"
                    } else {
                        "kvm"
                    },
                ]);
            }
        }

        crate::run_command(&mut command)
    }
}
