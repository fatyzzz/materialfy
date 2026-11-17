#!/usr/bin/env python3
"""Install Qt from the download.qt.io online repository without the installer or aqtinstall.

aqtinstall 3.3 does not understand the Qt 6.12 repository layout (qt6_6120/qt6_6120_<arch>), so we
read Updates.xml ourselves: take the qt.qt6.<ver>.<arch> package plus the requested addons, download
all of their DownloadableArchives, verify SHA-1 and extract the 7z archives into one prefix. Qt 6
archives are relocatable, so no qt.conf or path patching is needed.

    python3 install_qt.py --out ~/Qt/6.12.0/gcc_64 --addons qtshadertools qtimageformats

Only the qmake path goes to stdout (the log goes to stderr). --github appends QTDIR/QMAKE to
$GITHUB_ENV and bin/ to $GITHUB_PATH. If --out already holds the same package set (the
.materialfy-qt marker), nothing is downloaded - this is what makes the CI cache work.
"""
from __future__ import annotations

import argparse
import hashlib
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
import xml.etree.ElementTree as ET
from pathlib import Path

BASE = "https://download.qt.io/online/qtsdkrepository"
MARKER = ".materialfy-qt"

# host -> (OS directory in the repository, default arch, package name suffix)
HOSTS = {
    "windows": ("windows_x86", "msvc2022_64", "win64_msvc2022_64"),
    "linux": ("linux_x64", "linux_gcc_64", "linux_gcc_64"),
    "linux_arm64": ("linux_arm64", "linux_gcc_arm64", "linux_gcc_arm64"),
    # clang_64 is a universal (x86_64 + arm64) build
    "mac": ("mac_x64", "clang_64", "clang_64"),
    # Android target builds; they also need a desktop Qt of the same version as QT_HOST_PATH
    "android": ("all_os", "arm64_v8a", "android_arm64_v8a"),
}


def log(msg: str) -> None:
    print(f"[install_qt] {msg}", file=sys.stderr, flush=True)


def fetch_with_curl(url: str, dest: Path | None) -> bytes | None:
    """Download through the system curl, which verifies with the OS certificate store; None on failure."""
    cmd = ["curl", "-fsSL", "--retry", "3", "-A", "materialfy-ci", url]
    if dest is not None:
        cmd += ["-o", str(dest)]
    result = subprocess.run(cmd, capture_output=True)
    if result.returncode != 0:
        log(f"curl failed for {url}: {result.stderr.decode(errors='replace').strip()}")
        return None
    return b"" if dest is not None else result.stdout


def fetch(url: str, dest: Path | None = None, attempts: int = 5) -> bytes:
    err: Exception | None = None
    for i in range(1, attempts + 1):
        try:
            req = urllib.request.Request(url, headers={"User-Agent": "materialfy-ci"})
            with urllib.request.urlopen(req, timeout=120) as resp:
                if dest is None:
                    return resp.read()
                with open(dest, "wb") as f:
                    shutil.copyfileobj(resp, f, 1 << 20)
                return b""
        except urllib.error.HTTPError as e:
            if e.code == 404:
                raise
            err = e
        except (urllib.error.URLError, TimeoutError, ConnectionError, OSError) as e:
            err = e
            # Python's own certificate store can miss a mirror's intermediate certificate (seen on a
            # self-hosted Windows runner); the system curl completes the chain itself
            if "CERTIFICATE_VERIFY_FAILED" in str(e) and shutil.which("curl"):
                data = fetch_with_curl(url, dest)
                if data is not None:
                    return data
        log(f"failed {url}: {err}; attempt {i}/{attempts}")
        time.sleep(5 * i)
    raise RuntimeError(f"could not download {url}: {err}")


def repo_candidates(os_dir: str, ver: str, arch: str) -> list[str]:
    tag = f"qt6_{ver.replace('.', '')}"
    if os_dir == "all_os":
        return [f"{BASE}/all_os/android/{tag}/{tag}_{arch}"]
    # 6.8+: qt6_6120/qt6_6120_<arch> (Windows) or qt6_6120/qt6_6120 (Linux, macOS); older ones are flat
    return [
        f"{BASE}/{os_dir}/desktop/{tag}/{tag}_{arch}",
        f"{BASE}/{os_dir}/desktop/{tag}/{tag}",
        f"{BASE}/{os_dir}/desktop/{tag}",
    ]


def load_repo(os_dir: str, ver: str, arch: str) -> tuple[str, ET.Element]:
    for url in repo_candidates(os_dir, ver, arch):
        try:
            data = fetch(f"{url}/Updates.xml")
        except urllib.error.HTTPError:
            continue
        log(f"repository: {url}")
        return url, ET.fromstring(data)
    raise SystemExit(f"Updates.xml for Qt {ver} ({os_dir}, {arch}) not found")


def find_7z() -> str:
    for name in ("7z", "7zz", "7za"):
        exe = shutil.which(name)
        if exe:
            return exe
    for path in (r"C:\Program Files\7-Zip\7z.exe", "/opt/homebrew/bin/7zz", "/usr/local/bin/7zz"):
        if Path(path).exists():
            return path
    raise SystemExit("7-Zip is required: 7z (Windows, p7zip-full / 7zip) or 7zz (brew install sevenzip)")


def sha1_of(path: Path) -> str:
    h = hashlib.sha1()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main() -> int:
    # Windows consoles may default to a legacy code page
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", errors="replace")
    host_default = {"Windows": "windows", "Darwin": "mac"}.get(platform.system(), "linux")
    if host_default == "linux" and platform.machine() in ("aarch64", "arm64"):
        host_default = "linux_arm64"
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--host", choices=HOSTS, default=host_default)
    ap.add_argument("--version", default="6.12.0")
    ap.add_argument("--arch", help="repository arch directory (msvc2022_64, linux_gcc_64, linux_gcc_arm64, clang_64; "
                    "arm64_v8a or x86_64 with --host android)")
    ap.add_argument("--addons", nargs="*", default=["qtshadertools", "qtimageformats"])
    ap.add_argument("--out", required=True, type=Path, help="Qt prefix (will contain bin/, lib/, qml/ ...)")
    ap.add_argument("--skip", nargs="*", default=["qtdoc"], help="archive name prefixes to skip")
    ap.add_argument("--github", action="store_true", help="export QTDIR/QMAKE/PATH for GitHub Actions")
    ap.add_argument("--dry-run", action="store_true", help="only list the archives")
    args = ap.parse_args()

    os_dir, arch_default, _ = HOSTS[args.host]
    arch = args.arch or arch_default
    if args.host == "android":
        suffix = f"android_{arch}"
    else:
        suffix = HOSTS[args.host][2] if arch == arch_default else (f"win64_{arch}" if args.host == "windows" else arch)
    vtag = args.version.replace(".", "")
    names = [f"qt.qt6.{vtag}.{suffix}"] + [f"qt.qt6.{vtag}.addons.{a}.{suffix}" for a in args.addons]

    out: Path = args.out.expanduser().resolve()
    stamp = "\n".join([args.version, arch, *names, *sorted(args.skip)])
    marker = out / MARKER
    qmake = out / "bin" / ("qmake6.exe" if args.host == "windows" else "qmake6")
    if not qmake.exists():
        qmake = qmake.with_name("qmake.exe" if args.host == "windows" else "qmake")

    if marker.exists() and marker.read_text(encoding="utf-8") == stamp and qmake.exists():
        log(f"Qt is already installed in {out}")
    else:
        repo, root = load_repo(os_dir, args.version, arch)
        packages = {p.findtext("Name"): p for p in root.findall("PackageUpdate")}
        jobs: list[tuple[str, str]] = []
        for name in names:
            pkg = packages.get(name)
            if pkg is None:
                raise SystemExit(f"package {name} is not in the repository")
            version = pkg.findtext("Version") or ""
            archives = [a.strip() for a in (pkg.findtext("DownloadableArchives") or "").split(",") if a.strip()]
            for archive in archives:
                if any(archive.startswith(s) for s in args.skip):
                    continue
                jobs.append((archive, f"{repo}/{name}/{version}{archive}"))
        for archive, url in jobs:
            log(f"{archive} <- {url}")
        if args.dry_run:
            return 0

        seven = find_7z()
        if out.exists():
            shutil.rmtree(out)
        out.mkdir(parents=True)
        with tempfile.TemporaryDirectory(prefix="qt-dl-") as tmp:
            for archive, url in jobs:
                dest = Path(tmp) / archive
                fetch(url, dest)
                expected = fetch(f"{url}.sha1").decode().split()[0].strip().lower()
                actual = sha1_of(dest)
                if actual != expected:
                    raise SystemExit(f"SHA-1 mismatch for {archive}: {actual} != {expected}")
                cmd = [seven, "x", "-y", "-bd", "-bso0", f"-o{out}", str(dest)]
                # 7-Zip drops the symlinks in Qt archives (libQt6*.so -> .so.6) as "dangerous":
                # 25.x keeps them with -snld20, 21-24 with -snld, the old p7zip 16.02 knows neither flag
                for flags in (["-snld20"], ["-snld"], []):
                    if subprocess.run([*cmd, *flags]).returncode == 0:
                        break
                else:
                    raise SystemExit(f"7z could not extract {archive}")
                dest.unlink()
                log(f"extracted {archive}")
        # the ICU archive (Linux) lands in the prefix root, but Qt looks for it via RUNPATH $ORIGIN/../lib
        for icu in out.glob("libicu*.so*"):
            shutil.move(str(icu), out / "lib" / icu.name)
        if not qmake.exists():
            qmake = qmake.with_name("qmake.exe" if args.host == "windows" else "qmake")
        if not qmake.exists():
            raise SystemExit(f"{qmake} is missing after extraction")
        marker.write_text(stamp, encoding="utf-8")
        log(f"Qt {args.version} installed in {out}")

    if args.github:
        with open(os.environ["GITHUB_ENV"], "a", encoding="utf-8") as f:
            f.write(f"QTDIR={out}\nQMAKE={qmake}\nQt6_DIR={out}\n")
        with open(os.environ["GITHUB_PATH"], "a", encoding="utf-8") as f:
            f.write(f"{out / 'bin'}\n")
    print(qmake)
    return 0


if __name__ == "__main__":
    sys.exit(main())
