# 第三方组件许可声明 / Third-Party Notices

CIN（以下简称「本应用」）在安装包 `assets/toolchain/<abi>/` 内随附预编译的
`adb`、`fastboot` 及其动态依赖库，用于在本机直接与 Android 设备通信。
本文件列出这些组件的来源、版本与许可条款。

本应用自身源代码以 **GPL-3.0** 发布，见仓库根目录 `LICENSE`。
下文各组件的许可与 GPL-3.0 相互独立，随附方式遵循各自条款。

---

## 一、二进制来源

| 项 | 说明 |
|---|---|
| 上游 | Termux 项目 APT 仓库 (https://packages.termux.dev/apt/termux-main) |
| 主包 | `android-tools` (`adb`, `fastboot`) |
| 依赖包 | `abseil-cpp`, `brotli`, `fmt`, `libc++`, `liblz4`, `libprotobuf`, `zlib`, `zstd` |
| 拉取方式 | 构建时由 `scripts/fetch-toolchain.py` 下载 `.deb` 并校验 SHA256，**不提交进 Git 仓库** |
| 裁剪 | 仅保留 `adb` / `fastboot` 的 `DT_NEEDED` 依赖闭包，未使用组件已剔除 |
| 架构 | `arm64-v8a`, `armeabi-v7a`, `x86_64` |

构建产物对应的具体包版本与 SHA256 记录在 CI 日志与
`assets/toolchain/<abi>/manifest.txt` 中，可逐项核对。

---

## 二、组件与许可

### 1. android-tools (adb, fastboot)

- 上游项目：Android Open Source Project — `platform/system/core`
- 许可：**Apache License 2.0**
- 版权：Copyright (C) 2007-2024 The Android Open Source Project
- 全文：https://www.apache.org/licenses/LICENSE-2.0

### 2. abseil-cpp

- 上游：https://github.com/abseil/abseil-cpp
- 许可：**Apache License 2.0**
- 版权：Copyright 2017 The Abseil Authors
- 随附 `libabsl_*.so`（共 48 个）

### 3. libc++ (libc++_shared.so)

- 上游：https://libcxx.llvm.org/
- 许可：**Apache License 2.0 with LLVM Exceptions**
- 版权：Copyright (C) 2009-2024 LLVM Project
- 全文：https://llvm.org/LICENSE.txt

### 4. brotli (libbrotlicommon, libbrotlidec, libbrotlienc)

- 上游：https://github.com/google/brotli
- 许可：**MIT License**
- 版权：Copyright (c) 2009, 2010, 2013-2016 by the Brotli Authors

### 5. fmt (libfmt.so)

- 上游：https://github.com/fmtlib/fmt
- 许可：**MIT License**
- 版权：Copyright (c) 2012 - present, Victor Zverovich and {fmt} contributors

### 6. LZ4 (liblz4.so)

- 上游：https://github.com/lz4/lz4
- 许可：**BSD 2-Clause License**
- 版权：Copyright (c) 2011-2020, Yann Collet

### 7. Protocol Buffers (libprotobuf.so, libutf8_validity.so)

- 上游：https://github.com/protocolbuffers/protobuf
- 许可：**BSD 3-Clause License**
- 版权：Copyright 2008 Google Inc.

### 8. zlib (libz.so, libz.so.1)

- 上游：https://zlib.net/
- 许可：**zlib License**
- 版权：Copyright (C) 1995-2024 Jean-loup Gailly and Mark Adler

### 9. Zstandard (libzstd.so, libzstd.so.1)

- 上游：https://github.com/facebook/zstd
- 许可：**BSD 3-Clause License** 或 **GPL-2.0**（双许可）
- 本应用按 **BSD 3-Clause** 条款使用
- 版权：Copyright (c) Meta Platforms, Inc. and affiliates

---

## 三、明确未随附的内容

本应用**不包含、不分发**下列内容，它们是本应用的边界，不是遗漏：

1. 任何厂商签名的 `programmer.elf` / `DevprgProgrammer.elf` / firehose loader；
2. 任何厂商签名摘要、证书、私钥；
3. 任何厂商私有固件、OTA 包、分区镜像；
4. 任何用于绕过 Bootloader 锁定或签名校验的代码。

EDL（Emergency Download）刷写所需的 programmer 文件属于设备厂商资产，
其分发权不归于本应用。若用户需要该能力，须自行向设备厂商获取合法来源的文件，
并在应用内通过「导入本地文件」功能指定，文件始终保存在用户本机。

---

## 四、关于「仅供学习交流」

本应用不依赖「仅供学习交流」作为免责声明。上述组件均为开源软件，
其许可条款明确授予再分发权，本应用只需履行署名与许可全文随附义务，
已在本文档中逐项履行。

---

## 五、许可全文获取

各许可全文可随源码获取：

- Apache-2.0 — https://www.apache.org/licenses/LICENSE-2.0.txt
- MIT — https://opensource.org/license/mit
- BSD-2-Clause — https://opensource.org/license/bsd-2-clause
- BSD-3-Clause — https://opensource.org/license/bsd-3-clause
- zlib — https://zlib.net/zlib_license.html
- LLVM Exception — https://llvm.org/LICENSE.txt

如需离线副本，请提交 Issue，我们会在仓库 `docs/licenses/` 下补齐全文。

---

*本文档由 `scripts/fetch-toolchain.py` 的实际打包清单生成，
与 APK 内 `assets/toolchain/<abi>/manifest.txt` 一一对应。*
