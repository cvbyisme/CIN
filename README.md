# CIN

面向 Android 设备的工具集。同一套 Rust 核心，两种形态：

- **`cin` CLI** —— 跑在 Termux 或任意 Linux 主机上；
- **CIN App** —— Material 3 图形界面，破坏性操作前弹出原生弹窗确认。

> **状态：早期开发。** CLI 已支持设备发现；App 目前有「设备列表 + 弹窗确认 + 重启」，
> 并且 **adb / fastboot 已随应用内置，开箱可用**。EDL 刷写链路尚未实现。

## 仓库结构

```text
core/          Rust 核心库 + cin CLI（零第三方依赖，jni 仅在 android 目标下引入）
app/           Android App（Kotlin + Jetpack Compose + Material 3）
.github/       CI（fmt/clippy/test）、Release（三架构 CLI）、Android（出 debug APK）
```

## 它是什么

CIN 把 `adb` / `fastboot` 的调用、输出解析与流程编排收敛到一套可测试的 Rust 核心上：

- **核心零依赖**：CLI 只用 `std`，编译快、体积小、行为可预期；
- **逻辑与执行分离**：所有进程调用都在 `CommandRunner` trait 后面，因此解析逻辑可以在 PC 上以纯函数单测，不需要真机；
- **内置公开工具链，不内置厂商私有二进制**：`adb` / `fastboot` 及依赖库随 APK 分发；
  厂商 `programmer.elf` 等签名资产一律不内置、不下载，只能由用户从本地导入。

## 它不是什么

- **不是一键刷机神器。** CIN 不预置任何厂商 `programmer.elf`、签名摘要或私有固件；
- **不绕过授权。** 不做 BL 解锁、不做签名校验绕过；
- **不代为获取 programmer。** EDL 所需的 firehose loader 需你自行向厂商获取，应用只提供导入入口，不提供下载；
- **不保证你的设备不出问题。** 刷机本质是高危操作，作者不承担任何后果。

## 弹窗确认是怎么保证可信的

这是 App 形态最重要的一条设计约束：**弹窗里显示的命令，和真正执行的命令，必须是同一份数据**。

```text
Kotlin 点击"重启到 BL"
   ↓
Rust: reboot::plan(target, protocol, serial) -> Plan { render, commands, consequences, risk }
   ↓                                    ↓
   ↓ render（文本）                ↓ commands（argv 数组）
ConfirmDialog 展示            Runner.runAll() 逐条执行
```

- `Plan.render()` 生成的正文直接显示在 Dialog 里，`Plan.commands` 直接交给 `ProcessBuilder`；
- Kotlin 侧**没有任何拼接命令字符串的代码**，因此不可能出现"提示 A、执行 B"；
- 执行用 `ProcessBuilder(listOf(...))` 而非 `sh -c "..."`，参数里的空格、引号、分号都不会被解释。

## 安装

### CLI：从 Release 下载

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

### App：从 Actions 取 debug APK

APK 目前只出 debug 版，由 [Android 工作流](.github/workflows/android.yml) 构建：

1. 打开 Actions → **Android** → 最近一次成功的运行；
2. 下载产物 `cin-debug-apk`；
3. `adb install cin-debug.apk`。

工作流做的事：用 NDK r27c 交叉编译 `arm64-v8a` / `armeabi-v7a` / `x86_64` 三个 `libcin.so`，
注入 `app/app/src/main/jniLibs/`，再由 Gradle 打包。

> **adb / fastboot 已内置。** CI 构建时从 Termux 仓库拉取 `android-tools` 及其依赖，
> 按 `DT_NEEDED` 闭包裁剪后打进 `assets/toolchain/<abi>/`，首次启动自动展开到
> `/data/data/com.cvby.cin/files/cin/toolchain/`，**无需用户准备任何东西**。
>
> 代价是 APK 体积：三个 ABI 合计约 36.6 MB（arm64-v8a 13.8 / armeabi-v7a 8.7 / x86_64 14.2）。
> 二进制不进 Git 仓库，由 `scripts/fetch-toolchain.py` 在构建时下载并做 SHA256 校验。
> 详见 [docs/toolchain.md](docs/toolchain.md)。

### 从源码构建

```sh
cd core
cargo build --release
./target/release/cin version
```

交叉编译到 Android、以及构建 App 的 APK，详见 [docs/building.md](docs/building.md)。

## 用法

### CLI

```sh
cin help          # 帮助
cin version       # 版本与目标架构
cin devices       # 列出 adb / fastboot 设备
```

`cin devices` 会分别调用 `adb devices -l` 与 `fastboot devices`，把结果归一化成统一的 `Device` 结构。
模式判定覆盖 `device` / `unauthorized` / `offline` / `sideload` / `fastboot` / `fastbootd` / `edl`。

### App

1. 打开应用，点右上角刷新扫描设备；
2. 设备卡片上显示序列号与当前模式（adb / fastboot / 未授权…）；
3. 点「重启」「到 BL」「到 Rec」任一按钮；
4. **弹出确认窗**，列出风险等级、后果、将要执行的确切命令；
5. 点「确认」才真正执行，点「取消」什么都不发生。

顶栏还有个 **EDL Loader** 入口（芯片图标）：

- 从本地文件选择器导入你自己的 `programmer.elf`；
- 导入时计算 SHA-256 与读取 ELF 架构，弹窗展示便于你核对；
- 文件存在应用私有目录 `files/cin/loaders/`，可随时删除。

> **注意：目前只能导入，不能刷写。** EDL 刷写链路尚未实现，
> 导入后的 loader 不会触发任何操作。本应用不会替你下载 programmer。

## 环境变量

| 变量 | 说明 | 默认 |
|---|---|---|
| `CIN_HOME` | 数据目录，工具链放在 `$CIN_HOME/toolchain` | `$HOME/.cin` |

## 路线图

- [x] 设备发现与状态解析（adb / fastboot / sideload）
- [x] 确认弹窗逻辑（`Plan` / `Risk` / `render`，纯函数可测）
- [x] `reboot` 目标映射（system / bootloader / fastbootd / recovery / edl）
- [x] `flash` 计划构造与分区名校验
- [x] `libcin.so` + JNI，App 与 CLI 复用同一套核心
- [ ] `cin getvar` 读取 bootloader 变量
- [ ] CLI 接入 `reboot` / `flash` 子命令
- [ ] `cin payload` 解包 `payload.bin`
- [x] 内置 adb / fastboot 工具链（三 ABI，CI 构建时拉取）
- [x] App 内 EDL loader 导入入口（导入 / 校验 / 列表）
- [ ] EDL 刷写链路（`qdl` 接入，需 programmer）
- [ ] CLI 侧 `cin toolchain install`
- [ ] App 内分区镜像选择与刷写流程

## 开发

```sh
cd core
cargo test                       # 全部单测（无需真机）
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings

# JNI 层只在 android 目标下编译，需要单独检查
rustup target add x86_64-linux-android
cargo clippy --target x86_64-linux-android --lib -- -D warnings
```

CI 会在每次推送时跑 fmt / clippy（含 android 目标）/ test；打 `v*` 标签时自动交叉编译三个 Android 目标并发布 CLI Release。
App 由独立的 Android 工作流构建，产出 debug APK。

## 许可

[GPL-3.0-or-later](LICENSE)

APK 内含的第三方预编译组件（`android-tools`、`abseil-cpp`、`libc++`、`brotli`、`fmt`、`lz4`、
`protobuf`、`zlib`、`zstd`）的来源、版本与许可，见
**[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)**。

## 致谢

设计上参考了 Termux、AOSP `platform-tools`、`payload-dumper-go` 等开源项目的公开接口与行为，代码为独立实现。
