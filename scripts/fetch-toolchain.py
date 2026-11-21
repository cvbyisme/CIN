#!/usr/bin/env python3
"""从 Termux 仓库拉取 adb / fastboot 及其依赖，裁剪后放进 APK 的 assets。

为什么是这套做法（详见 docs/toolchain.md）：

- Google 的 `platform-tools` 只发布 linux / darwin / windows 的 x86_64 版本，
  没有 Android 版，所以不能直接拿来用。
- Termux 的 `android-tools` 是 AOSP 源码为 Android 交叉编译的产物，
  许可清晰（Apache-2.0）且版本可固定。
- 它动态链接若干第三方库，这些库在 Termux 仓库里都能单独下载并校验。
- android-tools 的递归闭包有上百个 .so（abseil 拆得很碎），
  所以这里按 ELF 的 DT_NEEDED 做闭包裁剪，只保留真正用得到的。
- 系统自带的 libc.so / libm.so / libdl.so / liblog.so 不打包，运行时由 linker 解析。

用法：
    fetch-toolchain.py --abi arm64-v8a --out app/app/src/main/assets/toolchain
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import os
import shutil
import struct
import sys
import tarfile
import urllib.request
from pathlib import Path

# ---------------------------------------------------------------- 配置

MIRROR = os.environ.get(
    "TERMUX_MIRROR", "https://packages.termux.dev/apt/termux-main"
)

ABI_TO_ARCH = {
    "arm64-v8a": "aarch64",
    "armeabi-v7a": "arm",
    "x86_64": "x86_64",
}

MAIN_PACKAGE = "android-tools"

SYSTEM_LIBS = {
    "libc.so",
    "libm.so",
    "libdl.so",
    "liblog.so",
    "libandroid.so",
}

# ---------------------------------------------------------------- 下载


def fetch(url: str, timeout: int = 120) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": "cin-toolchain-fetch"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.read()
    except Exception as e:
        sys.exit(f"[fetch] 下载失败 {url}\n  {e}")


def load_index(arch: str) -> dict[str, dict[str, str]]:
    url = f"{MIRROR}/dists/stable/main/binary-{arch}/Packages.gz"
    print(f"[index] {url}")
    raw = gzip.decompress(fetch(url)).decode("utf-8", "replace")

    packages: dict[str, dict[str, str]] = {}
    cur: dict[str, str] = {}
    for line in raw.splitlines():
        if not line.strip():
            if "Package" in cur:
                packages[cur["Package"]] = cur
            cur = {}
            continue
        if line.startswith(" ") or ":" not in line:
            continue
        k, v = line.split(":", 1)
        cur[k.strip()] = v.strip()
    if "Package" in cur:
        packages[cur["Package"]] = cur

    print(f"[index] {len(packages)} 个包")
    return packages


def download_package(index: dict[str, dict[str, str]], name: str) -> bytes:
    meta = index.get(name)
    if meta is None:
        sys.exit(f"[fetch] 索引里没有包 {name}")

    data = fetch(f"{MIRROR}/{meta['Filename']}")

    expect = meta.get("SHA256", "")
    actual = hashlib.sha256(data).hexdigest()
    if expect and actual != expect:
        sys.exit(f"[fetch] {name} SHA256 不匹配\n  期望 {expect}\n  实际 {actual}")

    print(f"[fetch] {name} {meta.get('Version', '?')} ({len(data) / 1024:.0f} KB)")
    return data

# ---------------------------------------------------------------- deb 解包


def extract_deb(data: bytes) -> dict[str, bytes]:
    pos = 8
    data_tar = None
    while pos + 60 <= len(data):
        header = data[pos : pos + 60]
        name = header[:16].decode("ascii", "replace").strip()
        size_str = header[48:58].decode("ascii", "replace").strip()
        if not size_str.isdigit():
            break
        size = int(size_str)
        body = data[pos + 60 : pos + 60 + size]
        pos += 60 + size + (size & 1)
        if name.startswith("data.tar"):
            data_tar = body
            break

    if data_tar is None:
        sys.exit("[extract] .deb 里没有 data.tar.*")

    out: dict[str, bytes] = {}
    links: dict[str, str] = {}  # 软链路径 -> 目标路径
    last_err = ""
    for mode in ("r:gz", "r:xz", "r:"):
        try:
            out.clear()
            links.clear()
            with tarfile.open(fileobj=io.BytesIO(data_tar), mode=mode) as tf:
                for m in tf.getmembers():
                    nm = m.name
                    if nm.startswith("./"):
                        nm = nm[2:]
                    if m.issym():
                        # deb 里 libz.so / libz.so.1 都是指向 libz.so.1.4 的软链。
                        # 必须解析成真实内容，否则运行时找不到 SONAME。
                        links[nm] = os.path.normpath(
                            os.path.join(os.path.dirname(nm), m.linkname)
                        )
                        continue
                    if not m.isfile():
                        continue
                    f = tf.extractfile(m)
                    if f is None:
                        continue
                    out[nm] = f.read()

            # 把软链展开成真实内容
            for link_path, target in links.items():
                blob = out.get(target)
                if blob is None:
                    # 目标不在同一个包里，跳过；依赖解析阶段会去别的包找
                    continue
                out[link_path] = blob
            return out
        except Exception as e:
            last_err = str(e)
            continue

    sys.exit(f"[extract] 无法解包 data.tar（可能用了 zstd）: {last_err}")

# ---------------------------------------------------------------- ELF 解析


def elf_needed(blob: bytes) -> list[str]:
    if len(blob) < 64 or blob[:4] != b"\x7fELF":
        return []

    is64 = blob[4] == 2
    try:
        if is64:
            (e_shoff,) = struct.unpack_from("<Q", blob, 0x28)
            e_shentsize, e_shnum, e_shstrndx = struct.unpack_from("<HHH", blob, 0x3A)
        else:
            (e_shoff,) = struct.unpack_from("<I", blob, 0x20)
            e_shentsize, e_shnum, e_shstrndx = struct.unpack_from("<HHH", blob, 0x2E)
    except struct.error:
        return []

    if e_shoff == 0 or e_shnum == 0 or e_shstrndx >= e_shnum:
        return []

    secs = []
    for i in range(e_shnum):
        off = e_shoff + i * e_shentsize
        try:
            if is64:
                nm, typ, _fl, _ad, soff, ssz = struct.unpack_from("<IIQQQQ", blob, off)
            else:
                nm, typ, _fl, _ad, soff, ssz = struct.unpack_from("<IIIIII", blob, off)
        except struct.error:
            return []
        secs.append({"nm": nm, "typ": typ, "off": soff, "sz": ssz})

    shstr_off = secs[e_shstrndx]["off"]

    def sec_name(n: int) -> str:
        end = blob.find(b"\x00", shstr_off + n)
        if end < 0:
            return ""
        return blob[shstr_off + n : end].decode("ascii", "replace")

    dyn = dynstr = None
    for s in secs:
        nm = sec_name(s["nm"])
        if nm == ".dynamic":
            dyn = s
        elif nm == ".dynstr":
            dynstr = s
    if dyn is None or dynstr is None:
        return []

    str_off = dynstr["off"]
    ent = 16 if is64 else 8
    fmt = "<qQ" if is64 else "<iI"

    needed: list[str] = []
    for i in range(dyn["sz"] // ent):
        try:
            tag, val = struct.unpack_from(fmt, blob, dyn["off"] + i * ent)
        except struct.error:
            break
        if tag == 0:
            break
        if tag == 1:
            end = blob.find(b"\x00", str_off + val)
            if end > 0:
                needed.append(blob[str_off + val : end].decode("ascii", "replace"))
    return needed

def elf_soname(blob: bytes) -> str | None:
    """读出 ELF 的 DT_SONAME（动态库自报的名字）。"""
    if len(blob) < 64 or blob[:4] != b"\x7fELF":
        return None

    is64 = blob[4] == 2
    try:
        if is64:
            (e_shoff,) = struct.unpack_from("<Q", blob, 0x28)
            e_shentsize, e_shnum, e_shstrndx = struct.unpack_from("<HHH", blob, 0x3A)
        else:
            (e_shoff,) = struct.unpack_from("<I", blob, 0x20)
            e_shentsize, e_shnum, e_shstrndx = struct.unpack_from("<HHH", blob, 0x2E)
    except struct.error:
        return None

    if e_shoff == 0 or e_shnum == 0 or e_shstrndx >= e_shnum:
        return None

    secs = []
    for i in range(e_shnum):
        off = e_shoff + i * e_shentsize
        try:
            if is64:
                nm, _t, _f, _a, soff, ssz = struct.unpack_from("<IIQQQQ", blob, off)
            else:
                nm, _t, _f, _a, soff, ssz = struct.unpack_from("<IIIIII", blob, off)
        except struct.error:
            return None
        secs.append({"nm": nm, "off": soff, "sz": ssz})

    shstr_off = secs[e_shstrndx]["off"]

    def sec_name(n: int) -> str:
        end = blob.find(b"\x00", shstr_off + n)
        if end < 0:
            return ""
        return blob[shstr_off + n : end].decode("ascii", "replace")

    dyn = dynstr = None
    for s in secs:
        nm = sec_name(s["nm"])
        if nm == ".dynamic":
            dyn = s
        elif nm == ".dynstr":
            dynstr = s
    if dyn is None or dynstr is None:
        return None

    str_off = dynstr["off"]
    ent = 16 if is64 else 8
    fmt = "<qQ" if is64 else "<iI"

    for i in range(dyn["sz"] // ent):
        try:
            tag, val = struct.unpack_from(fmt, blob, dyn["off"] + i * ent)
        except struct.error:
            return None
        if tag == 0:
            break
        if tag == 14:  # DT_SONAME
            end = blob.find(b"\x00", str_off + val)
            if end > 0:
                return blob[str_off + val : end].decode("ascii", "replace")
    return None


def verify(bins: dict[str, bytes], libs: dict[str, bytes]) -> None:
    """确保所有 DT_NEEDED 都能在 libs 或系统库里找到。

    这是运行时的唯一防线：少一个库，设备上就是 CANNOT LINK EXECUTABLE。
    """
    available = set(libs)

    # 每个库的 SONAME 必须有同名文件，否则 linker 按 SONAME 找不到
    for name, blob in libs.items():
        sn = elf_soname(blob)
        if sn and sn not in available:
            sys.exit(f"[verify] {name} 的 SONAME={sn} 没有对应文件")

    missing: list[str] = []
    for name, blob in list(bins.items()) + list(libs.items()):
        for dep in elf_needed(blob):
            if dep in available or dep in SYSTEM_LIBS:
                continue
            missing.append(f"{dep} <- {name}")

    if missing:
        sys.exit("[verify] 以下依赖无法满足：" + chr(10) + "  " + (chr(10) + "  ").join(sorted(missing)))

    print(f"[verify] {len(libs)} 个库的依赖全部满足")


# ---------------------------------------------------------------- 主流程


def collect(arch: str) -> tuple[dict[str, bytes], dict[str, bytes]]:
    index = load_index(arch)

    pool: dict[str, bytes] = {}
    scanned: set[str] = set()

    def scan_package(pkg: str) -> None:
        if pkg in scanned or pkg not in index:
            return
        scanned.add(pkg)
        files = extract_deb(download_package(index, pkg))
        for path, blob in files.items():
            base = os.path.basename(path)
            if base.endswith(".so") or ".so." in base:
                pool.setdefault(base, blob)

    scan_package(MAIN_PACKAGE)
    main_files = extract_deb(download_package(index, MAIN_PACKAGE))

    bins: dict[str, bytes] = {}
    for name in ("adb", "fastboot"):
        for path, blob in main_files.items():
            if path.endswith(f"/bin/{name}"):
                bins[name] = blob
                break
        if name not in bins:
            sys.exit(f"[collect] {MAIN_PACKAGE} 里找不到 {name}")

    print(
        f"[collect] adb {len(bins['adb']) / 1024:.0f} KB, "
        f"fastboot {len(bins['fastboot']) / 1024:.0f} KB"
    )

    deps = [
        d.split("(")[0].strip()
        for d in index[MAIN_PACKAGE].get("Depends", "").split(",")
        if d.strip()
    ]

    libs: dict[str, bytes] = {}
    pending: set[str] = set()
    for blob in bins.values():
        pending.update(elf_needed(blob))

    for _ in range(16):
        missing = [
            n for n in sorted(pending) if n not in libs and n not in SYSTEM_LIBS
        ]
        if not missing:
            break

        progress = False
        for lib in missing:
            if lib not in pool:
                for pkg in deps:
                    scan_package(pkg)
                    if lib in pool:
                        break
            if lib in pool:
                libs[lib] = pool[lib]
                progress = True
                print(f"[dep] {lib} ({len(libs[lib]) / 1024:.0f} KB)")

        if not progress:
            unresolved = [n for n in missing if n not in libs]
            sys.exit(f"[collect] 无法解析以下库：{unresolved}")

        pending = set()
        for blob in libs.values():
            pending.update(elf_needed(blob))

    for name in list(libs):
        if name in SYSTEM_LIBS:
            del libs[name]

    verify(bins, libs)
    return bins, libs


def write_out(
    bins: dict[str, bytes], libs: dict[str, bytes], out_dir: Path, arch: str
) -> None:
    bin_dir = out_dir / "bin"
    lib_dir = out_dir / "lib"
    bin_dir.mkdir(parents=True, exist_ok=True)
    lib_dir.mkdir(parents=True, exist_ok=True)

    for name, blob in bins.items():
        p = bin_dir / name
        p.write_bytes(blob)
        p.chmod(0o755)

    for name, blob in libs.items():
        p = lib_dir / name
        p.write_bytes(blob)
        p.chmod(0o755)
        if ".so." in name:
            alias_p = lib_dir / (name.split(".so.")[0] + ".so")
            if not alias_p.exists():
                shutil.copyfile(p, alias_p)

    # Android 的 AssetManager 没有目录枚举 API，必须在构建期生成清单，
    # 由 Kotlin 侧逐行读取再 assets.open()。
    entries = [f"bin/{n}" for n in sorted(bins)]
    entries += [f"lib/{n}" for n in sorted({p.name for p in lib_dir.iterdir()})]
    manifest = out_dir / "manifest.txt"
    manifest.write_text("\n".join(entries) + "\n", encoding="utf-8")

    total = sum(len(b) for b in bins.values()) + sum(len(b) for b in libs.values())
    print(
        f"[out] {arch}: {len(bins)} bins + {len(libs)} libs "
        f"(+{len(entries) - len(bins) - len(libs)} alias) = "
        f"{total / 1048576:.2f} MB -> {out_dir}"
    )
    print(f"[out] manifest: {len(entries)} 项 -> {manifest}")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--abi", action="append", default=None, help="目标 ABI，可重复")
    ap.add_argument(
        "--out",
        default="app/app/src/main/assets/toolchain",
        help="输出根目录（每个 ABI 一个子目录）",
    )
    args = ap.parse_args()

    abis = args.abi or list(ABI_TO_ARCH)
    out_root = Path(args.out)

    for abi in abis:
        arch = ABI_TO_ARCH.get(abi)
        if arch is None:
            sys.exit(f"[main] 不认识的 ABI: {abi}")
        print(f"\n===== {abi} ({arch}) =====")
        bins, libs = collect(arch)
        write_out(bins, libs, out_root / abi, arch)

    print("\n完成。")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
