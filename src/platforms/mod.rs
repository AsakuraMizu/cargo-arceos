use axconfig_gen::Config;
use strum::{AsRefStr, Display, EnumString, VariantNames};

#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, EnumString, VariantNames, AsRefStr, Display)]
#[strum(serialize_all = "kebab-case")]
pub enum Platform {
    Dummy,
    AARCH64_BSTA1000B,
    AARCH64_PHYTIUM_PI,
    AARCH64_QEMU_VIRT,
    AARCH64_RASPI4,
    LOONGARCH64_QEMU_VIRT,
    RISCV64_QEMU_VIRT,
    #[strum(to_string = "x86_64-pc-oslab")]
    X86_64_PC_OSLAB,
    #[strum(to_string = "x86_64-qemu-q35")]
    X86_64_QEMU_Q35,
}

impl From<Platform> for Config {
    fn from(platform: Platform) -> Config {
        let mut config =
            Config::from_toml(include_str!("defconfig.toml")).expect("base config is invalid");
        let plat = Config::from_toml(match platform {
            Platform::Dummy => include_str!("dummy.toml"),
            Platform::AARCH64_BSTA1000B => include_str!("aarch64-bsta1000b.toml"),
            Platform::AARCH64_PHYTIUM_PI => include_str!("aarch64-phytium-pi.toml"),
            Platform::AARCH64_QEMU_VIRT => include_str!("aarch64-qemu-virt.toml"),
            Platform::AARCH64_RASPI4 => include_str!("aarch64-raspi4.toml"),
            Platform::LOONGARCH64_QEMU_VIRT => include_str!("loongarch64-qemu-virt.toml"),
            Platform::RISCV64_QEMU_VIRT => include_str!("riscv64-qemu-virt.toml"),
            Platform::X86_64_PC_OSLAB => include_str!("x86_64-pc-oslab.toml"),
            Platform::X86_64_QEMU_Q35 => include_str!("x86_64-qemu-q35.toml"),
        })
        .expect("platform config is invalid");
        config.merge(&plat).expect("failed to load built-in config");

        config
    }
}

#[derive(Debug, Clone, Copy, EnumString, VariantNames, AsRefStr, Display)]
#[strum(serialize_all = "kebab-case")]
pub enum Arch {
    Aarch64,
    Loongarch64,
    Riscv64,
    #[strum(to_string = "x86_64")]
    X86_64,
}

impl From<Arch> for Platform {
    fn from(arch: Arch) -> Self {
        match arch {
            Arch::Aarch64 => Self::AARCH64_QEMU_VIRT,
            Arch::Loongarch64 => Self::LOONGARCH64_QEMU_VIRT,
            Arch::Riscv64 => Self::RISCV64_QEMU_VIRT,
            Arch::X86_64 => Self::X86_64_QEMU_Q35,
        }
    }
}

impl From<Platform> for Arch {
    fn from(platform: Platform) -> Self {
        match platform {
            Platform::AARCH64_BSTA1000B
            | Platform::AARCH64_PHYTIUM_PI
            | Platform::AARCH64_QEMU_VIRT
            | Platform::AARCH64_RASPI4 => Self::Aarch64,
            Platform::LOONGARCH64_QEMU_VIRT => Self::Loongarch64,
            Platform::RISCV64_QEMU_VIRT => Self::Riscv64,
            Platform::X86_64_PC_OSLAB | Platform::X86_64_QEMU_Q35 => Self::X86_64,
            Platform::Dummy => Self::X86_64,
        }
    }
}
