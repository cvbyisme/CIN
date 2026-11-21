//! CIN 核心库。
//!
//! 设计原则：
//! 1. **不直接执行任何命令**——所有外部调用都通过 [`shell::CommandRunner`] 抽象，
//!    这样核心逻辑可以完全在 PC 上用假实现做单测。
//! 2. **不做 I/O 副作用**——解析、构造、编排都是纯函数或纯结构。
//! 3. **不假设运行在 Android**——只要 `.cin` 能在 Termux 跑，就同时能在 x86_64 上跑。

pub mod confirm;
pub mod context;
pub mod device;
pub mod flash;
pub mod reboot;
pub mod shell;

/// JNI 导出层，只在 Android 目标下编译。
#[cfg(target_os = "android")]
pub mod jni;

pub mod toolchain {
    //! 工具链定位与元信息。

    use std::path::{Path, PathBuf};

    /// 工具链目录约定：
    ///
    /// ```text
    /// <root>/bin/adb, fastboot
    /// <root>/lib/libxxx.so
    /// ```
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Toolchain {
        root: PathBuf,
    }

    impl Toolchain {
        pub fn new(root: impl AsRef<Path>) -> Self {
            Self {
                root: root.as_ref().to_path_buf(),
            }
        }

        pub fn root(&self) -> &Path {
            &self.root
        }

        pub fn bin_dir(&self) -> PathBuf {
            self.root.join("bin")
        }

        pub fn lib_dir(&self) -> PathBuf {
            self.root.join("lib")
        }

        pub fn binary(&self, name: &str) -> PathBuf {
            self.bin_dir().join(name)
        }

        /// 工具链是否看起来可用（至少 adb 与 fastboot 都在）。
        pub fn looks_usable(&self) -> bool {
            self.binary("adb").is_file() && self.binary("fastboot").is_file()
        }
    }

    /// 当前进程平台对应的工具链子目录名。
    pub fn platform_dir() -> &'static str {
        #[cfg(target_arch = "aarch64")]
        {
            "aarch64"
        }
        #[cfg(target_arch = "arm")]
        {
            "armv7"
        }
        #[cfg(target_arch = "x86_64")]
        {
            "x86_64"
        }
        #[cfg(target_arch = "x86")]
        {
            "x86"
        }
        #[cfg(not(any(
            target_arch = "aarch64",
            target_arch = "arm",
            target_arch = "x86_64",
            target_arch = "x86"
        )))]
        {
            "unknown"
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn paths_are_joined_under_root() {
            let tc = Toolchain::new("/data/cin/toolchain");
            assert_eq!(tc.bin_dir(), PathBuf::from("/data/cin/toolchain/bin"));
            assert_eq!(tc.lib_dir(), PathBuf::from("/data/cin/toolchain/lib"));
            assert_eq!(
                tc.binary("fastboot"),
                PathBuf::from("/data/cin/toolchain/bin/fastboot")
            );
        }

        #[test]
        fn missing_toolchain_is_not_usable() {
            let tc = Toolchain::new("/definitely/not/here");
            assert!(!tc.looks_usable());
        }

        #[test]
        fn platform_dir_is_known() {
            assert_ne!(platform_dir(), "unknown", "构建目标架构未被识别");
        }
    }
}
