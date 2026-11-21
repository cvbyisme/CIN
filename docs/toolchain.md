# 工具链从哪来，为什么这么选

CIN 的 APK 里自带 `adb` 与 `fastboot`，装上即可列设备、无需用户自己准备二进制。
这份文档说明这些文件从哪来、为什么不能用更“官方”的源、以及体积与许可的取舍。

## 一、先说一个否定结论

**Google 的 `platform-tools` 不能用于 Android 设备。**

它只发布三个平台的产物：

```
platform-tools-latest-linux.zip      x86_64 + glibc
platform-tools-latest-darwin.zip     x86_64 + arm64 (macOS)
platform-tools-latest-windows.zip    x86_64 (PE)
```

这些是给 PC 连手机用的。它们的 `adb` 是**为桌面系统编译**的，链接 glibc，
在 Android 的 bionic 上根本无法执行。

所以“CI 从 Google 官方源下载 platform-tools 打进 APK”这条路从一开始就不成立。
不存在“镜像”问题——是官方根本没出这个产物。

## 二、可行的来源

需要的是**为 Android 交叉编译的 `adb` / `fastboot`**。可选做法：

| 方案 | 来源 | 优点 | 缺点 |
|---|---|---|---|
| A | 用 NDK 从 AOSP 源码自己编 | 完全可控，可编静态版 | CI 首次 20–40 分钟，维护成本高 |
| **B（本项目）** | **Termux 的 `android-tools`** | **几分钟、版本可固定、SHA256 可校验** | **动态链接，需一并打包依赖库** |
| C | 从他人的 APK 里提取 | 无 | 来源不明、无许可证、无法验证；不采用 |

选 B。

`android-tools` 包的元信息（可直接核对）：

```
Package: android-tools
Version: 37.0.0p1-3
Architecture: aarch64
Depends: abseil-cpp, brotli, fmt, libc++, liblz4,
         libprotobuf, pcre2, zlib, zstd
```

### 为什么不带 Termux 一起跑

Termux 的二进制硬编码了 `/data/data/com.termux/files/usr/...` 路径。
要“带上 Termux”意味着：

- 打包整个 Termux 前缀（100 MB+）；
- 跨应用读取另一个 app 的私有目录（普通应用无权）；
- 或要求用户先装 Termux（违背开箱可用）。

所以只取二进制与库，不要 Termux 的运行时。实测证明这样做可行：

```
adb      -> interp = /system/bin/linker64
            依赖 13 个库，其中 4 个系统自带（libc/libdl/liblog/libz）
fastboot -> interp = /system/bin/linker64
            依赖  6 个库，其中 4 个系统自带
```

`linker64` 是 Android 自己的动态链接器，系统自带。我们只需把剩下 9 个库打包进去。

## 三、依赖裁剪：为什么 109 个 .so 最后只剩 58 个

`android-tools` 的间接依赖非常多：abseil 被拆成了近 100 个独立 `.so`。
如果全打进去，一个 ABI 就要 10.76 MB 的库。

脚本 `scripts/fetch-toolchain.py` 的做法是**按 ELF 的 `DT_NEEDED` 做闭包裁剪**：

1. 读 `adb` / `fastboot` 的 `DT_NEEDED`；
2. 对每个库再读它的 `DT_NEEDED`，递归到不动点；
3. 只保留真正可达的库。

结果：114 个降为 58 个。

## 四、两个真实的坑（都实际踩过）

### 1. `libz.so.1` 必须打包，`libz.so` 不能

Android 系统里只有 `libz.so`，**没有 `libz.so.1`**。而 Termux 的 adb 需要的是
带版本号的 `libz.so.1`。

如果系统库名单写成前缀匹配（`libz.*`），`libz.so.1` 会被误当作系统库过滤掉，
结果就是设备上 `CANNOT LINK EXECUTABLE "adb": library "libz.so.1" not found`。

所以 `SYSTEM_LIBS` 是精确名单，注释里写明了这件事。

### 2. deb 里的 `.so` 很多是软链接

`libz.so` / `libz.so.1` 是指向 `libz.so.1.4` 的软链接，真实内容只在后者。
`tarfile` 的 `isfile()` 会跳过软链接，如果只解普通文件，这些 SONAME 就丢了。

脚本现在会解析软链接、把真实内容写入每个名字，并在最后**复制**一份无版本别名
（不建软链：APK 解压后软链不可靠）。

## 五、构建期自检

`write_out` 之前会跑 `verify()`：

- 每个库的 `DT_SONAME` 必须有同名文件；
- 所有 `DT_NEEDED` 必须能在包内或系统库里解析。

任一不满足就直接失败。这样依赖一旦变化，CI 不会静默产出一个跑不起来的 APK。

## 六、实测量（37.0.0p1-3）

| ABI | 二进制 | 库 | 体积 |
|---|---|---|---|
| arm64-v8a | adb + fastboot | 58 | 13.76 MB |
| armeabi-v7a | adb + fastboot | 58 | 8.68 MB |
| x86_64 | adb + fastboot | 58 | 14.20 MB |
| **合计** | 6 | 174 | **36.64 MB** |

APK 本体（含 Rust `libcin.so`）约 14 MB，加大后约 50 MB 量级。

## 七、运行时如何生效

APK 里的 assets 不是真实文件路径，不能 `execve`，也不能当库搜索目录。

所以 `Toolchain.kt` 在首次启动时：

1. 读 `assets/toolchain/<abi>/manifest.txt`（AssetManager 没有目录枚举 API）；
2. 逐行复制到 `<files>/cin/toolchain/`；
3. `chmod 755`；
4. 自检 `adb` / `fastboot` 存在且可执行；
5. 写 `.stamp`（内容为 `versionCode:abi`），下次启动直接短路。

执行时 `Runner` 会设置：

```
PATH              = <files>/cin/toolchain/bin:...
LD_LIBRARY_PATH   = <files>/cin/toolchain/lib:...
```

`LD_LIBRARY_PATH` 是必须的：Android 10+ 不会搜索应用私有目录里的 `.so`。

## 八、许可

| 组件 | 许可 |
|---|---|
| adb / fastboot（android-tools） | Apache-2.0 |
| abseil-cpp | Apache-2.0 |
| libc++_shared | Apache-2.0 with LLVM exception |
| brotli | MIT |
| fmt | MIT |
| liblz4 | BSD-2-Clause |
| libprotobuf | BSD-3-Clause |
| zlib | zlib License |
| zstd | BSD-3-Clause / GPL-2.0 双许可 |

完整清单见 `THIRD_PARTY_NOTICES.md`。

## 九、本项目不收什么

**不内置任何 `programmer.elf` / `DevprgProgrammer.elf` / Firehose loader。**

这类文件不是公开工具链：它们是每个机型/平台专属的签名引导程序，
通常从厂商固件包里提取，版权状态不明，也与特定设备强绑定。

EDL 能力因此只做到“能让设备进 9008”（`adb reboot edl`），不做刷写。
用户若自行使用，需自备与其设备匹配的 loader。
CIN 不代用户承担这部分的分发责任。

## 十、CI 如何验证（real APK 实测）

构建完成后由 `scripts/check-apk.py` 直接拆 APK 做双向校验，任一不满足即构建失败：

1. `lib/<abi>/libcin.so` 三个架构都在；
2. 每个 ABI 的工具链条目数必须是 **63**（2 bin + 60 lib + 1 manifest）；
3. `bin/adb`、`bin/fastboot`、`manifest.txt` 逐个点名检查；
4. **双向一致性**：manifest 列出的文件都真实存在；存在的文件都列在 manifest 里；
5. **依赖闭包**：每个 `.so` 的 `DT_NEEDED` 要么在 `lib/` 里，要么是系统库。

第 3~4 项是重点。以前的 CI 只跑 `unzip -l | grep 'lib/.*\.so'`，
而 `adb` / `fastboot` 没有扩展名、`manifest.txt` 也不是 `.so`，
刚好全部落在正则之外——真漏了也看不见。而 `Toolchain.kt` 恰恰是靠
`manifest.txt` 逐行展开的，它一缺，应用就是「装得上、用不了」。

本地也能自己跑：

```sh
python3 scripts/check-apk.py app-debug.apk
```

### 实测结果（commit 198a91f 的 CI 产物）

```text
APK: app-debug.apk (92.4 MB)
=== libcin.so: 3 个 ===
  lib/arm64-v8a/libcin.so
  lib/armeabi-v7a/libcin.so
  lib/x86_64/libcin.so
=== 工具链 ===
arm64-v8a: 63 个条目
  ok   bin/adb  3.4 MB
  ok   bin/fastboot  2.3 MB
  manifest 62 项, 实际文件 62 个
  ok   依赖闭包完整
armeabi-v7a: 63 个条目
  ok   bin/adb  2.0 MB
  ok   bin/fastboot  1.3 MB
  manifest 62 项, 实际文件 62 个
  ok   依赖闭包完整
x86_64: 63 个条目
  ok   bin/adb  3.6 MB
  ok   bin/fastboot  2.4 MB
  manifest 62 项, 实际文件 62 个
  ok   依赖闭包完整
=== APK 内容校验通过 ===
```

顺带确认了 `noCompress` 真的生效：APK 里所有工具链条目的时间戳都是
`1981-01-01 01:01`（ZIP 的 stored 标志），没有被 deflate。

提醒：这里只验证了「装包内容完整」，并不意味着已过真机验证。
JNI 调用与工具链实际执行仍需真机人工确认。
