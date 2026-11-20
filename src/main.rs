//! CIN 命令行入口。
//!
//! 目前只实现「列设备」，用来验证工具链与解析链路；
//! 其余子命令（flash / reboot / getvar）在后续提交中逐步接入。

use std::env;
use std::process::ExitCode;

use cin_core::device::{self, Device};
use cin_core::shell::{Command, CommandRunner, ShellEnv, StdCommandRunner};
use cin_core::toolchain::{platform_dir, Toolchain};

const USAGE: &str = "\
CIN — Android device toolkit

USAGE:
    cin <COMMAND>

COMMANDS:
    devices        列出当前连接的 adb 与 fastboot 设备
    version        打印版本信息
    help           显示本帮助

ENVIRONMENT:
    CIN_HOME       数据目录，默认 $HOME/.cin
";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("help") | Some("-h") | Some("--help") => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some("version") | Some("-V") | Some("--version") => {
            println!("cin {}", env!("CARGO_PKG_VERSION"));
            println!("toolchain platform: {}", platform_dir());
            ExitCode::SUCCESS
        }
        Some("devices") => run_devices(),
        Some(other) => {
            eprintln!("未知命令: {other}");
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run_devices() -> ExitCode {
    let home = env::var("CIN_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::PathBuf::from(env::var("HOME").unwrap_or_else(|_| ".".into())).join(".cin")
        });
    let toolchain = Toolchain::new(home.join("toolchain"));
    let shell_env = ShellEnv::new(home.clone(), home.join("tmp"), toolchain.bin_dir())
        .with_lib(toolchain.lib_dir());

    for dir in shell_env.dirs() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("无法创建目录 {}: {e}", dir.display());
            return ExitCode::FAILURE;
        }
    }

    if !toolchain.looks_usable() {
        eprintln!(
            "未找到工具链：{}（首次使用请运行 `cin toolchain install`）",
            toolchain.root().display()
        );
        return ExitCode::FAILURE;
    }

    let runner = StdCommandRunner::new();
    let mut found: Vec<Device> = Vec::new();

    for (binary, parser) in [
        ("adb", device::parse_adb_devices as fn(&str) -> Vec<Device>),
        (
            "fastboot",
            device::parse_fastboot_devices as fn(&str) -> Vec<Device>,
        ),
    ] {
        let argv = if binary == "adb" {
            vec![binary, "devices", "-l"]
        } else {
            vec![binary, "devices"]
        };
        let cmd = Command::new(argv);
        match runner.run(&cmd, &shell_env, &mut |_| {}) {
            Ok(out) if out.ok() => found.extend(parser(&out.stdout())),
            Ok(out) => eprintln!(
                "{} 返回非零状态 {}：{}",
                binary,
                out.code,
                out.stderr().lines().next().unwrap_or("")
            ),
            Err(e) => eprintln!("无法执行 {binary}: {e}"),
        }
    }

    if found.is_empty() {
        println!("未发现设备。请检查 OTG / 数据线连接，并确认设备已开启 USB 调试。");
        return ExitCode::SUCCESS;
    }

    println!("{:<24} {:<12} STATE", "SERIAL", "MODE");
    for d in &found {
        println!(
            "{:<24} {:<12} {}",
            d.serial,
            format!("{:?}", d.mode),
            d.raw_state
        );
    }
    ExitCode::SUCCESS
}
