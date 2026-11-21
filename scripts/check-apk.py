#!/usr/bin/env python3
"""校验 APK 里的工具链是否完整。

CI 专用，也方便本地手动跑：
    python3 scripts/check-apk.py app-debug.apk

为什么需要这个脚本：以前 CI 只看一眼 lib 目录下的 .so 条目，
结果 adb/fastboot（没有扩展名）和 manifest.txt 根本不在匹配范围内——
真的漏了也发现不了。而 Toolchain.kt 完全依赖 manifest.txt 才能展开工具链，
所以这里把双向一致性也查一遍。
"""

import os
import sys
import tempfile
import zipfile

SYSTEM_LIBS = {"libc.so", "libm.so", "libdl.so", "liblog.so", "libandroid.so"}
ABIS = ["arm64-v8a", "armeabi-v7a", "x86_64"]
EXPECTED_BINS = ["adb", "fastboot"]


def size_human(n: int) -> str:
    for unit in ("B", "KB", "MB", "GB"):
        if n < 1024:
            return f"{n:.1f} {unit}"
        n /= 1024
    return f"{n:.1f} TB"


def main(argv) -> int:
    if len(argv) != 2:
        print("用法: check-apk.py <apk>")
        return 2
    apk = argv[1]
    if not os.path.isfile(apk):
        print(f"!!! APK 不存在: {apk}")
        return 1

    print(f"APK: {apk} ({size_human(os.path.getsize(apk))})")
    problems = []

    with zipfile.ZipFile(apk) as z:
        names = z.namelist()

        # 1) JNI 库：每个 ABI 都必须有 libcin.so
        jni = [n for n in names if n.startswith("lib/") and n.endswith("libcin.so")]
        print(f"\n=== libcin.so: {len(jni)} 个 ===")
        for n in sorted(jni):
            print(f"  {n}")
        if not jni:
            problems.append("APK 里没有 libcin.so")

        # 2) 每个 ABI 的工具链：2 bin + 60 lib + 1 manifest = 63
        print("\n=== 工具链 ===")
        for abi in ABIS:
            prefix = f"assets/toolchain/{abi}/"
            entries = [n for n in names if n.startswith(prefix)]
            print(f"{abi}: {len(entries)} 个条目")
            if not entries:
                problems.append(f"{abi}: 整个工具链缺失")
                continue

            for b in EXPECTED_BINS:
                p = f"{prefix}bin/{b}"
                if p in names:
                    print(f"  ok   bin/{b}  {size_human(z.getinfo(p).file_size)}")
                else:
                    print(f"  MISS bin/{b}")
                    problems.append(f"{abi}: 缺 bin/{b}")

            mf = f"{prefix}manifest.txt"
            if mf not in names:
                print("  MISS manifest.txt")
                problems.append(f"{abi}: 缺 manifest.txt（Toolchain.kt 依赖它，缺了无法展开）")
                continue

            # 3) 双向一致性：manifest 列出的都存在，存在的都在 manifest 里
            listed = [l.strip() for l in z.read(mf).decode("utf-8").splitlines() if l.strip()]
            missing = [e for e in listed if f"{prefix}{e}" not in names]
            actual = [
                n[len(prefix):]
                for n in entries
                if n.startswith(prefix + "bin/") or n.startswith(prefix + "lib/")
            ]
            extra = [a for a in actual if a not in listed]
            print(f"  manifest {len(listed)} 项, 实际文件 {len(actual)} 个")
            if missing:
                print(f"  MISS manifest 中列出但 APK 缺失: {missing[:5]}")
                problems.append(f"{abi}: manifest 有 {len(missing)} 项不存在")
            if extra:
                print(f"  WARN APK 中存在但未列入 manifest: {extra[:5]}")
                problems.append(f"{abi}: {len(extra)} 个文件未列入 manifest")

            # 4) 依赖闭包：每个 DT_NEEDED 要么在 lib/ 里，要么是系统库
            lib_names = {a.split("/")[-1] for a in actual if a.startswith("lib/")}
            broken = []
            for a in actual:
                if not a.startswith("lib/"):
                    continue
                p = f"{prefix}{a}"
                with tempfile.NamedTemporaryFile(delete=False) as tmp:
                    tmp.write(z.read(p))
                    tmp_path = tmp.name
                try:
                    need = elf_needed(tmp_path)
                finally:
                    os.unlink(tmp_path)
                for d in need:
                    if d not in lib_names and d not in SYSTEM_LIBS:
                        broken.append((a, d))
            if broken:
                print(f"  MISS 依赖无法解析: {broken[:5]}")
                problems.append(f"{abi}: {len(broken)} 个依赖无法解析")
            else:
                print("  ok   依赖闭包完整")

    print()
    if problems:
        print("!!! 校验失败：")
        for p in problems:
            print(f"  - {p}")
        return 1
    print("=== APK 内容校验通过 ===")
    return 0


def elf_needed(path: str):
    """极简 ELF 解析：只取 DT_NEEDED。够用即可，不引第三方库。"""
    import struct

    with open(path, "rb") as f:
        d = f.read()
    if d[:4] != b"\x7fELF":
        return []
    is64 = d[4] == 2
    end = "<" if d[5] == 1 else ">"
    if is64:
        phoff = struct.unpack_from(end + "Q", d, 0x20)[0]
        phentsize = struct.unpack_from(end + "H", d, 0x36)[0]
        phnum = struct.unpack_from(end + "H", d, 0x38)[0]
    else:
        phoff = struct.unpack_from(end + "I", d, 0x1C)[0]
        phentsize = struct.unpack_from(end + "H", d, 0x2A)[0]
        phnum = struct.unpack_from(end + "H", d, 0x2C)[0]

    dyn_off = dyn_sz = None
    for i in range(phnum):
        off = phoff + i * phentsize
        if is64:
            t = struct.unpack_from(end + "I", d, off)[0]
            o = struct.unpack_from(end + "Q", d, off + 8)[0]
            s = struct.unpack_from(end + "Q", d, off + 32)[0]
        else:
            t = struct.unpack_from(end + "I", d, off)[0]
            o = struct.unpack_from(end + "I", d, off + 4)[0]
            s = struct.unpack_from(end + "I", d, off + 16)[0]
        if t == 2:
            dyn_off, dyn_sz = o, s
    if dyn_off is None:
        return []

    step = 16 if is64 else 8
    ents = []
    for p in range(dyn_off, dyn_off + dyn_sz, step):
        if is64:
            tag, val = struct.unpack_from(end + "qQ", d, p)
        else:
            tag, val = struct.unpack_from(end + "iI", d, p)
        ents.append((tag, val))
        if tag == 0:
            break

    strtab = next((v for t, v in ents if t == 5), None)
    if strtab is None:
        return []
    out = []
    for t, v in ents:
        if t == 1:
            out.append(d[strtab + v :].split(b"\x00")[0].decode("utf-8", "replace"))
    return out


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
