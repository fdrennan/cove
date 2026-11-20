#!/usr/bin/env bash
# A sysroot of another architecture's GTK/WebKitGTK 4.1, for build-linux.sh to
# cross-link the desktop App panel's embedded browser (webview.rs) - without
# touching this machine's packages: no dpkg --add-architecture, no apt.
#
# Operator, 2026-11-20: "local needs embedded browser badly we have to keep the
# experience in the ade" - the aarch64 Cove had shipped without the embedded
# App panel since 2026-10-15, because this host has no arm64 WebKitGTK to
# link against.
#
#   packaging/linux/cross-sysroot.sh aarch64     -> ~/.cache/cove-sysroot/aarch64
#
# Reads Ubuntu's own package index for this host's release (ports.ubuntu.com
# for arm64), takes the -dev packages the webview links, and everything they
# depend on that is a library or its headers (never libc: zig supplies glibc
# 2.28), and unpacks each .deb, checked against the index's SHA-256, into the
# sysroot. Downloads are cached; a second run fetches nothing new.
set -euo pipefail
arch="${1:-aarch64}"
case "$arch" in
  aarch64) deb_arch=arm64; mirror="http://ports.ubuntu.com/ubuntu-ports" ;;
  x86_64)  deb_arch=amd64; mirror="http://archive.ubuntu.com/ubuntu" ;;
  *) echo "no sysroot recipe for $arch" >&2; exit 1 ;;
esac
. /etc/os-release
codename="${COVE_SYSROOT_RELEASE:-$VERSION_CODENAME}"
root="${COVE_SYSROOT:-$HOME/.cache/cove-sysroot/$arch}"
cache="$HOME/.cache/cove-sysroot/debs-$deb_arch-$codename"
mkdir -p "$root" "$cache"

python3 - "$mirror" "$codename" "$deb_arch" "$cache" "$root" <<'PY'
import hashlib, lzma, os, re, subprocess, sys, urllib.request

mirror, codename, arch, cache, root = sys.argv[1:]
want = ["libwebkit2gtk-4.1-dev", "libgtk-3-dev", "libsoup-3.0-dev", "libjavascriptcoregtk-4.1-dev", "libglib2.0-dev"]
# zig links against its own glibc 2.28; the host's compilers and tools are
# not wanted in a sysroot.
skip = re.compile(r"^(libc6|libc-dev|libc6-dev|linux-libc-dev|libgcc|libstdc\+\+|gcc|cpp|g\+\+|perl|python3|dpkg|debconf|pkgconf|pkg-config|bash|coreutils|dash|libcrypt-dev|libcrypt1)")

pkgs = {}
for comp in ("main", "universe"):
    idx = os.path.join(cache, f"Packages-{comp}")
    if not os.path.exists(idx):
        url = f"{mirror}/dists/{codename}/{comp}/binary-{arch}/Packages.xz"
        print(f"index {url}", flush=True)
        with urllib.request.urlopen(url, timeout=120) as r:
            open(idx, "wb").write(lzma.decompress(r.read()))
    for block in open(idx, encoding="utf-8", errors="replace").read().split("\n\n"):
        f = {}
        for line in block.splitlines():
            if line and not line[0].isspace() and ":" in line:
                k, v = line.split(":", 1)
                f[k] = v.strip()
        if "Package" in f:
            pkgs.setdefault(f["Package"], f)
            for prov in f.get("Provides", "").split(","):
                name = prov.strip().split(" ")[0]
                if name:
                    pkgs.setdefault(name, f)

seen, order, todo = set(), [], list(want)
while todo:
    n = todo.pop()
    if n in seen or skip.match(n):
        continue
    seen.add(n)
    f = pkgs.get(n)
    if not f:
        print(f"  (not in the index: {n})", flush=True)
        continue
    if f["Package"] != n and f["Package"] in seen:
        continue
    seen.add(f["Package"])
    order.append(f)
    for dep in re.split(r",", f.get("Depends", "")):
        alt = dep.split("|")[0].strip().split(" ")[0].split(":")[0]
        if alt and (alt.startswith("lib") or alt.endswith("-dev") or alt.startswith("gir1.2")):
            todo.append(alt)

print(f"{len(order)} packages for the {arch} sysroot", flush=True)
for f in order:
    deb = os.path.join(cache, os.path.basename(f["Filename"]))
    if not os.path.exists(deb):
        urllib.request.urlretrieve(f"{mirror}/{f['Filename']}", deb + ".part")
        os.replace(deb + ".part", deb)
    if hashlib.sha256(open(deb, "rb").read()).hexdigest() != f["SHA256"]:
        os.remove(deb)
        sys.exit(f"sha256 mismatch for {f['Filename']} - removed; run again")
    subprocess.run(["dpkg-deb", "-x", deb, root], check=True)
PY

# Absolute symlinks (libfoo.so -> /usr/lib/...) must point inside the sysroot.
find "$root" -type l | while read -r l; do
  t="$(readlink "$l")"
  case "$t" in /*) ln -sfn "$root$t" "$l" ;; esac
done
echo "✓ $root"
