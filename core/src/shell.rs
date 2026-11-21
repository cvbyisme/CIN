//! 命令抽象与执行。
//!
//! 核心只依赖 [`CommandRunner`] trait；[`StdCommandRunner`] 是唯一真正 spawn 进程的实现。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 一次调用需要的运行环境。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellEnv {
    pub home: PathBuf,
    pub tmp: PathBuf,
    pub toolchain_bin: PathBuf,
    pub toolchain_lib: PathBuf,
    pub extra_lib_dirs: Vec<PathBuf>,
    pub extra_vars: BTreeMap<String, String>,
}

impl ShellEnv {
    pub fn new(home: impl Into<PathBuf>, tmp: impl Into<PathBuf>, bin: impl Into<PathBuf>) -> Self {
        Self {
            home: home.into(),
            tmp: tmp.into(),
            toolchain_bin: bin.into(),
            toolchain_lib: PathBuf::new(),
            extra_lib_dirs: Vec::new(),
            extra_vars: BTreeMap::new(),
        }
    }

    pub fn with_lib(mut self, lib: impl Into<PathBuf>) -> Self {
        self.toolchain_lib = lib.into();
        self
    }

    /// 生成 PATH。工具链 bin 永远排第一，保证不会误用系统里的旧 adb。
    pub fn path_value(&self, base: &str) -> String {
        let mut parts: Vec<String> = Vec::new();
        push_unique(
            &mut parts,
            self.toolchain_bin.to_string_lossy().into_owned(),
        );
        for p in base.split(':').filter(|s| !s.is_empty()) {
            push_unique(&mut parts, p.to_string());
        }
        parts.join(":")
    }

    /// 生成 LD_LIBRARY_PATH。
    pub fn lib_path_value(&self, base: &str) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !self.toolchain_lib.as_os_str().is_empty() {
            push_unique(
                &mut parts,
                self.toolchain_lib.to_string_lossy().into_owned(),
            );
        }
        for d in &self.extra_lib_dirs {
            push_unique(&mut parts, d.to_string_lossy().into_owned());
        }
        for p in base.split(':').filter(|s| !s.is_empty()) {
            push_unique(&mut parts, p.to_string());
        }
        parts.join(":")
    }

    /// 在给定基础环境上叠加本环境。
    pub fn overlay(&self, base: BTreeMap<String, String>) -> BTreeMap<String, String> {
        let mut env = base;
        let path_base = env.get("PATH").cloned().unwrap_or_default();
        let lib_base = env.get("LD_LIBRARY_PATH").cloned().unwrap_or_default();
        env.insert("HOME".into(), self.home.to_string_lossy().into_owned());
        env.insert("TMPDIR".into(), self.tmp.to_string_lossy().into_owned());
        env.insert("PATH".into(), self.path_value(&path_base));
        env.insert("LD_LIBRARY_PATH".into(), self.lib_path_value(&lib_base));
        for (k, v) in &self.extra_vars {
            env.insert(k.clone(), v.clone());
        }
        env
    }

    /// 需要的工作目录，调用方负责创建。
    pub fn dirs(&self) -> Vec<&Path> {
        vec![self.home.as_path(), self.tmp.as_path()]
    }
}

fn push_unique(list: &mut Vec<String>, value: String) {
    let trimmed = value.trim_end_matches('/').to_string();
    if !trimmed.is_empty() && !list.iter().any(|v| v == &trimmed) {
        list.push(trimmed);
    }
}

/// 待执行命令。
///
/// 有意区分「普通」「root」「脚本」三种形态，而不是把 root 当成一个布尔开关散落各处。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// 直接执行，argv[0] 会在 PATH 中查找。
    Direct { argv: Vec<String> },
    /// `su -c <argv...>`
    Root { argv: Vec<String> },
    /// `sh -c <script>`；`as_root` 时变成 `su -c <script>`。
    Script { script: String, as_root: bool },
}

impl Command {
    pub fn new<I, S>(argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let argv: Vec<String> = argv.into_iter().map(Into::into).collect();
        assert!(!argv.is_empty(), "argv 不能为空");
        Self::Direct { argv }
    }

    pub fn root<I, S>(argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let argv: Vec<String> = argv.into_iter().map(Into::into).collect();
        assert!(!argv.is_empty(), "argv 不能为空");
        Self::Root { argv }
    }

    pub fn script(script: impl Into<String>) -> Self {
        Self::Script {
            script: script.into(),
            as_root: false,
        }
    }

    pub fn root_script(script: impl Into<String>) -> Self {
        Self::Script {
            script: script.into(),
            as_root: true,
        }
    }

    /// 最终交给操作系统的 argv。
    pub fn to_argv(&self) -> Vec<String> {
        match self {
            Command::Direct { argv } => argv.clone(),
            Command::Root { argv } => {
                let mut out = vec!["su".to_string(), "-c".to_string()];
                out.extend(argv.iter().cloned());
                out
            }
            Command::Script { script, as_root } => {
                let prefix = if *as_root { "su" } else { "sh" };
                vec![prefix.to_string(), "-c".to_string(), script.clone()]
            }
        }
    }

    pub fn label(&self) -> String {
        self.to_argv().first().cloned().unwrap_or_default()
    }
}

/// 一行输出。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputLine {
    pub is_stderr: bool,
    pub text: String,
}

/// 执行结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunOutput {
    pub code: i32,
    pub lines: Vec<OutputLine>,
}

impl RunOutput {
    pub fn ok(&self) -> bool {
        self.code == 0
    }

    pub fn stdout(&self) -> String {
        self.lines
            .iter()
            .filter(|l| !l.is_stderr)
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn stderr(&self) -> String {
        self.lines
            .iter()
            .filter(|l| l.is_stderr)
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// 执行器。测试里替换成 [`MockRunner`]。
pub trait CommandRunner {
    fn run(
        &self,
        cmd: &Command,
        env: &ShellEnv,
        on_line: &mut dyn FnMut(&OutputLine),
    ) -> std::io::Result<RunOutput>;
}

/// 真实实现：`std::process::Command`。
#[derive(Debug, Default, Clone, Copy)]
pub struct StdCommandRunner;

impl StdCommandRunner {
    pub fn new() -> Self {
        Self
    }
}

impl CommandRunner for StdCommandRunner {
    fn run(
        &self,
        cmd: &Command,
        env: &ShellEnv,
        on_line: &mut dyn FnMut(&OutputLine),
    ) -> std::io::Result<RunOutput> {
        use std::io::{BufRead, BufReader};
        use std::process::{Command as Proc, Stdio};

        let argv = cmd.to_argv();
        let mut builder = Proc::new(&argv[0]);
        builder.args(&argv[1..]);
        builder.current_dir(&env.tmp);
        builder.env_clear();
        for (k, v) in env.overlay(std::env::vars().collect()) {
            builder.env(k, v);
        }
        builder.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = builder.spawn()?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        // stderr 放到独立线程读，否则管道写满会互相阻塞。
        let err_handle = stderr.map(|s| {
            std::thread::spawn(move || -> Vec<OutputLine> {
                BufReader::new(s)
                    .lines()
                    .map_while(Result::ok)
                    .map(|text| OutputLine {
                        is_stderr: true,
                        text,
                    })
                    .collect()
            })
        });

        let mut lines: Vec<OutputLine> = Vec::new();
        if let Some(out) = stdout {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                let l = OutputLine {
                    is_stderr: false,
                    text: line,
                };
                on_line(&l);
                lines.push(l);
            }
        }
        if let Some(handle) = err_handle {
            if let Ok(err_lines) = handle.join() {
                for l in &err_lines {
                    on_line(l);
                }
                lines.extend(err_lines);
            }
        }

        let status = child.wait()?;
        Ok(RunOutput {
            code: status.code().unwrap_or(-1),
            lines,
        })
    }
}

/// 测试用的假执行器：按预设返回固定输出，并记录被调用的命令。
#[derive(Debug, Default)]
pub struct MockRunner {
    responses: std::sync::Mutex<Vec<(Vec<String>, RunOutput)>>,
    calls: std::sync::Mutex<Vec<Vec<String>>>,
}

impl MockRunner {
    pub fn new() -> Self {
        Self::default()
    }

    /// 当 argv 以 `prefix` 开头时返回 `output`。
    pub fn expect(&self, prefix: Vec<String>, output: RunOutput) -> &Self {
        self.responses.lock().unwrap().push((prefix, output));
        self
    }

    pub fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }
}

impl CommandRunner for MockRunner {
    fn run(
        &self,
        cmd: &Command,
        _env: &ShellEnv,
        on_line: &mut dyn FnMut(&OutputLine),
    ) -> std::io::Result<RunOutput> {
        let argv = cmd.to_argv();
        self.calls.lock().unwrap().push(argv.clone());

        let responses = self.responses.lock().unwrap();
        let matched = responses
            .iter()
            .find(|(prefix, _)| argv.starts_with(prefix))
            .map(|(_, out)| out.clone())
            .unwrap_or_default();

        for l in &matched.lines {
            on_line(l);
        }
        Ok(matched)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> ShellEnv {
        ShellEnv::new("/data/cin", "/data/cin/tmp", "/data/cin/toolchain/bin")
            .with_lib("/data/cin/toolchain/lib")
    }

    #[test]
    fn toolchain_bin_goes_first_in_path() {
        let path = env().path_value("/system/bin:/vendor/bin");
        let parts: Vec<&str> = path.split(':').collect();
        assert_eq!(parts[0], "/data/cin/toolchain/bin");
        assert_eq!(&parts[1..], &["/system/bin", "/vendor/bin"]);
    }

    #[test]
    fn path_deduplicates_and_strips_trailing_slash() {
        let e = env();
        let path = e.path_value("/data/cin/toolchain/bin/:/system/bin");
        assert_eq!(path.matches("/data/cin/toolchain/bin").count(), 1);
        assert!(!path.contains("bin//"));
    }

    #[test]
    fn lib_path_keeps_toolchain_first_and_unique() {
        let mut e = env();
        e.extra_lib_dirs.push(PathBuf::from("/vendor/lib64"));
        let libs = e.lib_path_value("/vendor/lib64:/system/lib64");
        let parts: Vec<&str> = libs.split(':').collect();
        assert_eq!(parts[0], "/data/cin/toolchain/lib");
        assert_eq!(parts.iter().filter(|p| **p == "/vendor/lib64").count(), 1);
    }

    #[test]
    fn overlay_sets_core_vars() {
        let mut base = BTreeMap::new();
        base.insert("PATH".to_string(), "/usr/bin".to_string());
        let out = env().overlay(base);
        assert_eq!(out["HOME"], "/data/cin");
        assert_eq!(out["TMPDIR"], "/data/cin/tmp");
        assert!(out["PATH"].starts_with("/data/cin/toolchain/bin"));
    }

    #[test]
    fn direct_command_argv_is_unchanged() {
        let c = Command::new(["fastboot", "devices"]);
        assert_eq!(c.to_argv(), vec!["fastboot", "devices"]);
        assert_eq!(c.label(), "fastboot");
    }

    #[test]
    fn root_command_wraps_with_su() {
        let c = Command::root(["id"]);
        assert_eq!(c.to_argv(), vec!["su", "-c", "id"]);
    }

    #[test]
    fn scripts_pick_shell_by_root_flag() {
        assert_eq!(
            Command::script("echo hi").to_argv(),
            vec!["sh", "-c", "echo hi"]
        );
        assert_eq!(
            Command::root_script("echo hi").to_argv(),
            vec!["su", "-c", "echo hi"]
        );
    }

    #[test]
    fn output_splits_streams() {
        let out = RunOutput {
            code: 0,
            lines: vec![
                OutputLine {
                    is_stderr: false,
                    text: "a".into(),
                },
                OutputLine {
                    is_stderr: true,
                    text: "b".into(),
                },
                OutputLine {
                    is_stderr: false,
                    text: "c".into(),
                },
            ],
        };
        assert_eq!(out.stdout(), "a\nc");
        assert_eq!(out.stderr(), "b");
        assert!(out.ok());
    }

    #[test]
    fn mock_runner_matches_by_prefix() {
        let mock = MockRunner::new();
        mock.expect(
            vec!["fastboot".into(), "devices".into()],
            RunOutput {
                code: 0,
                lines: vec![OutputLine {
                    is_stderr: false,
                    text: "abc\tfastboot".into(),
                }],
            },
        );
        let e = env();
        let out = mock
            .run(&Command::new(["fastboot", "devices"]), &e, &mut |_| {})
            .unwrap();
        assert_eq!(out.stdout(), "abc\tfastboot");
        assert_eq!(mock.calls().len(), 1);
    }
}
