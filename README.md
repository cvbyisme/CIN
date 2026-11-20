# CIN

面向 Android 设备的命令行工具集，用 Rust 编写，跑在手机上的 Termux 或任意 Linux 主机。

> **状态：早期开发。** 目前只实现了「设备发现」，其余能力按 [路线图](#路线图) 逐步加入。

## 它是什么

CIN 把 `adb` / `fastboot` 的调用、输出解析与流程编排收敛到一套可测试的 Rust 核心上：

- **核心零依赖**：只用 `std`，编译快、体积小、行为可预期；
- **逻辑与执行分离**：所有进程调用都在 `CommandRunner` trait 后面，因此解析逻辑可以在 PC 上以纯函数单测，不需要真机；
- **不内置厂商私有二进制**：工具链由用户自行获取，仓库只分发 `cin` 本体。

## 它不是什么

- **不是一键刷机神器。** CIN 不预置任何厂商 `programmer.elf`、签名摘要或私有固件；
- **不绕过授权。** 不做 BL 解锁、不做签名校验绕过；
- **不保证你的设备不出问题。** 刷机本质是高危操作，作者不承担任何后果。

## 安装

### 从 Release 下载

到 [Releases](https://github.com/cvbyisme/CIN/releases) 下载对应架构的压缩包：

| 架构 | 适用 |
|---|---|
| `arm64-v8a` | 绝大多数现代 Android 手机 |
| `armeabi-v7a` | 32 位老设备 |
| `x86_64` | 模拟器、部分平板 |

```sh
tar -xzf cin-v0.1.0-arm64-v8a.tar.gz
cd cin-v0.1.0-arm64-v8a
./cin version
```

### 从源码构建

```sh
cargo build --release
./target/release/cin version
```

交叉编译到 Android 需要 NDK，详见 [docs/building.md](docs/building.md)。

## 用法

```sh
cin help          # 帮助
cin version       # 版本与目标架构
cin devices       # 列出 adb / fastboot 设备
```

`cin devices` 会分别调用 `adb devices -l` 与 `fastboot devices`，把结果归一化成统一的 `Device` 结构：

```text
SERIAL                   MODE         STATE
R58M1234                 Adb          device
1234abcd                 AdbUnauthorized  unauthorized
abc123                   Fastboot     fastboot
```

模式判定覆盖 `device` / `unauthorized` / `offline` / `sideload` / `fastboot` / `fastbootd` 六种常见状态。

## 环境变量

| 变量 | 说明 | 默认 |
|---|---|---|
| `CIN_HOME` | 数据目录，工具链放在 `$CIN_HOME/toolchain` | `$HOME/.cin` |

## 路线图

- [x] 设备发现与状态解析（adb / fastboot / sideload）
- [ ] `cin getvar` 读取 bootloader 变量
- [ ] `cin reboot <target>`（system / bootloader / fastbootd / recovery / edl）
- [ ] `cin flash <partition> <image>`，含 `--slot=all`
- [ ] `cin payload` 解包 `payload.bin`
- [ ] 工具链引导安装（`cin toolchain install`）
- [ ] `libcin.so` + JNI，供图形界面复用同一套核心

## 开发

```sh
cargo test          # 全部单测（无需真机）
cargo clippy --all-targets
cargo fmt --check
```

CI 会在每次推送时跑 fmt / clippy / test；打 `v*` 标签时自动交叉编译三个 Android 目标并发布 Release。

## 许可

[GPL-3.0-or-later](LICENSE)

## 致谢

设计上参考了 Termux、AOSP `platform-tools`、`payload-dumper-go` 等开源项目的公开接口与行为，代码为独立实现。
