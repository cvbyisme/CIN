# 构建说明

仓库分两部分：`core/`（Rust）与 `app/`（Android）。

## CLI：本机构建（x86_64 / aarch64 Linux）

```sh
cd core
cargo build --release
./target/release/cin version
```

零依赖，只需要 stable Rust 即可。

## CLI：交叉编译到 Android

需要两样东西：

1. `rustup` 的 Android 目标；
2. Android NDK（用于提供 `clang` 链接器）。

```sh
rustup target add aarch64-linux-android
# 准备 NDK r27c（或更新），假设解压在 $NDK
export NDK=/opt/android-ndk-r27c
TOOLCHAIN=$NDK/toolchains/llvm/prebuilt/linux-x86_64

export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=$TOOLCHAIN/bin/aarch64-linux-android29-clang

cd core
cargo build --release --target aarch64-linux-android
```

产物在 `core/target/aarch64-linux-android/release/cin`。

其他架构同理：

| 目标 | 链接器 |
|---|---|
| `armv7-linux-androideabi` | `armv7a-linux-androideabi29-clang` |
| `x86_64-linux-android` | `x86_64-linux-android29-clang` |

## App：构建 debug APK

App 依赖 `libcin.so`，它必须先由 Rust 交叉编译出来，放进
`app/app/src/main/jniLibs/<abi>/libcin.so`，然后 Gradle 才能打包。

### 全自动（推荐）

推送到 `main` 后，`.github/workflows/android.yml` 会自动跑完整流程并上传 `cin-debug-apk` 产物。

### 本地手动

```sh
# 1. 三个 ABI 的 .so（省略 NDK 变量设置，同上）
for t in aarch64-linux-android:arm64-v8a armv7-linux-androideabi:armeabi-v7a x86_64-linux-android:x86_64; do
  target=${t%%:*}; abi=${t##*:}
  (cd core && cargo build --release --lib --target "$target")
  mkdir -p "app/app/src/main/jniLibs/$abi"
  cp "core/target/$target/release/libcin_core.so" "app/app/src/main/jniLibs/$abi/libcin.so"
done

# 2. 打包
cd app
gradle :app:assembleDebug
# 产物：app/build/outputs/apk/debug/app-debug.apk
```

### 为什么 .so 要改名

`core/Cargo.toml` 里

```toml
[lib]
name = "cin_core"
```

之所以不直接叫 `cin`，是因为 bin 也叫 `cin`，同名会让 `main.rs` 里的
`use cin::...` 解析到自身而编译失败。所以 Rust 产物是 `libcin_core.so`，
打包前改名成 `libcin.so`，Kotlin 侧 `System.loadLibrary("cin")` 就能加载。

## 为什么 CI 里用 NDK r27c

`aarch64-linux-android29-clang` 这一命名格式来自 NDK r23 之后的布局。r27c 是目前稳定且体积可控的版本，GitHub Actions 上有现成的 `nttld/setup-ndk` 缓存。

## 版本号

CLI 版本号来自 `core/Cargo.toml` 的 `version`；App 版本号来自
`app/app/build.gradle.kts` 的 `versionCode` / `versionName`。两处需手动保持一致。

## 打包

Release 工作流把 CLI 二进制打成 `cin-<tag>-<arch>.tar.gz`，内含一个同名目录，解压即可用：

```text
cin-v0.1.0-arm64-v8a/
└── cin
```