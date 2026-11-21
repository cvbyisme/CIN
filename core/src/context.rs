//! 把 env / toolchain / runner 装配在一起，供各子命令复用。

use std::path::PathBuf;

use crate::device::{self, Device};
use crate::shell::{Command, CommandRunner, RunOutput, ShellEnv, StdCommandRunner};
use crate::toolchain::Toolchain;

/// 一次 CLI 调用的上下文。
pub struct Context {
    pub home: PathBuf,
    pub toolchain: Toolchain,
    pub env: ShellEnv,
    pub runner: StdCommandRunner,
    /// `--yes`：跳过确认（只读操作本来就不问）。
    pub assume_yes: bool,
}

impl Context {
    pub fn new(home: Option<PathBuf>, assume_yes: bool) -> Self {
        let home = home.unwrap_or_else(default_home);
        let toolchain = Toolchain::new(home.join("toolchain"));
        let env = ShellEnv::new(home.clone(), home.join("tmp"), toolchain.bin_dir())
            .with_lib(toolchain.lib_dir());
        Self {
            home,
            toolchain,
            env,
            runner: StdCommandRunner::new(),
            assume_yes,
        }
    }

    /// 准备目录并检查工具链是否就位。
    pub fn prepare(&self) -> Result<(), String> {
        for dir in self.env.dirs() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("无法创建目录 {}: {e}", dir.display()))?;
        }
        if !self.toolchain.looks_usable() {
            return Err(format!(
                "未找到工具链：{}\n请把 adb 与 fastboot 放到 {}/bin 下。",
                self.toolchain.root().display(),
                self.toolchain.root().display()
            ));
        }
        Ok(())
    }

    pub fn run(&self, cmd: &Command) -> std::io::Result<RunOutput> {
        self.runner.run(cmd, &self.env, &mut |_| {})
    }

    pub fn run_with(
        &self,
        cmd: &Command,
        on_line: &mut dyn FnMut(&crate::shell::OutputLine),
    ) -> std::io::Result<RunOutput> {
        self.runner.run(cmd, &self.env, on_line)
    }

    /// 同时探测 adb 与 fastboot，合并结果。
    pub fn discover(&self) -> Vec<Device> {
        let mut found = Vec::new();

        if let Ok(out) = self.run(&Command::new(["adb", "devices", "-l"])) {
            if out.ok() {
                found.extend(device::parse_adb_devices(&out.stdout()));
            }
        }
        if let Ok(out) = self.run(&Command::new(["fastboot", "devices"])) {
            if out.ok() {
                found.extend(device::parse_fastboot_devices(&out.stdout()));
            }
        }
        found
    }

    /// 要求恰好一台设备，否则给出明确诊断。
    pub fn require_single_device(&self, want_fastboot: bool) -> Result<Device, String> {
        let candidates: Vec<Device> = self
            .discover()
            .into_iter()
            .filter(|d| {
                if want_fastboot {
                    d.is_fastboot()
                } else {
                    d.is_adb_usable()
                }
            })
            .collect();

        let expect = if want_fastboot { "fastboot" } else { "adb" };
        match candidates.len() {
            0 => Err(format!(
                "没有可用的 {expect} 设备。\n请检查连接，并确认设备已进入对应模式。"
            )),
            1 => Ok(candidates.into_iter().next().unwrap()),
            n => {
                let list = candidates
                    .iter()
                    .map(|d| format!("  {} ({:?})", d.serial, d.mode))
                    .collect::<Vec<_>>()
                    .join("\n");
                Err(format!(
                    "检测到 {n} 台 {expect} 设备，请只连接一台：\n{list}"
                ))
            }
        }
    }
}

fn default_home() -> PathBuf {
    if let Ok(h) = std::env::var("CIN_HOME") {
        return PathBuf::from(h);
    }
    let base = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(base).join(".cin")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 建一个当前用户可写、且保证为空的临时 home。
    ///
    /// 不能直接用 `/nonexistent-cin-test` 这类绝对路径：
    /// root 下 `create_dir_all` 会成功，非 root 下会 Permission denied，
    /// 导致测试结果随运行用户身份而变（CI 上就是这样翻车的）。
    fn temp_home(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cin-test-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn explicit_home_is_used_as_is() {
        let home = PathBuf::from("/tmp/cin-test");
        let ctx = Context::new(Some(home.clone()), false);
        assert_eq!(ctx.toolchain.root(), home.join("toolchain"));
        assert_eq!(ctx.env.home, home);
        assert!(!ctx.assume_yes);
    }

    #[test]
    fn prepare_fails_without_toolchain() {
        let home = temp_home("no-toolchain");
        let ctx = Context::new(Some(home.clone()), false);
        let err = ctx.prepare().unwrap_err();
        // 目录应当被建出来（说明 create_dir_all 成功），只是里面没有工具链
        assert!(home.is_dir(), "临时 home 未被创建: {}", home.display());
        assert!(err.contains("未找到工具链"), "err={err}");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn prepare_succeeds_when_toolchain_present() {
        let home = temp_home("with-toolchain");
        let bin = home.join("toolchain/bin");
        std::fs::create_dir_all(&bin).unwrap();
        for name in ["adb", "fastboot"] {
            std::fs::write(bin.join(name), b"").unwrap();
        }
        let ctx = Context::new(Some(home.clone()), false);
        assert!(ctx.prepare().is_ok(), "工具链齐全时 prepare 应成功");
        let _ = std::fs::remove_dir_all(&home);
    }
}
