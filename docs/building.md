# 构建说明

## 本机构建（x86_64 / aarch64 Linux）

```sh
cargo build --release
./target/release/cin version
```

零依赖，只需要 stable Rust 即可。

## 交叉编译到 Android

需要两样东西：

1. `rustup` 的 Android 目标；
2. Android NDK（用于提供 `clang` 链接器）。

```sh
rustup target add aarch64-linux-android
# 准备 NDK r27c（或更新），假设解压在 $NDK
export NDK=/opt/android-ndk-r27c
TOOLCHAIN=$NDK/toolchains/llvm/prebuilt/linux-x86_64

export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=$TOOLCHAIN/bin/aarch64-linux-android29-clang

cargo build --release --target aarch64-linux-android
```

产物在 `target/aarch64-linux-android/release/cin`。

其他架构同理：

| 目标 | 链接器 |
|---|---|
| `armv7-linux-androideabi` | `armv7a-linux-androideabi29-clang` |
| `x86_64-linux-android` | `x86_64-linux-android29-clang` |

## 为什么 CI 里用 NDK r27c

`aarch64-linux-android29-clang` 这一命名格式来自 NDK r23 之后的布局。r27c 是目前稳定且体积可控的版本，GitHub Actions 上有现成的 `nttld/setup-ndk` 缓存。

## 版本号

版本号来自 `Cargo.toml` 的 `version` 字段，Release 标签以 `v` 开头（如 `v0.1.0`）。两者需手动保持一致，后续会加脚本校验。

## 打包

Release 工作流把二进制打成 `cin-<tag>-<arch>.tar.gz`，内含一个同名目录，解压即可用：

```text
cin-v0.1.0-arm64-v8a/
└── cin
```