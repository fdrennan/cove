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

real, provided = {}, {}
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
            real[f["Package"]] = f
            for prov in f.get("Provides", "").split(","):
                name = prov.strip().split(" ")[0]
                if name:
                    provided.setdefault(name, f)
# A real package before one that only Provides its name: liblzma-dev stood
# for some other package's virtual name, and libtiff-4.pc's Requires found
# no liblzma.pc (2026-11-20).
pkgs = {**provided, **real}

def closure(names, seen):
    """The packages `names` need, by their declared dependencies, not yet in `seen`."""
    out, todo = [], list(names)
    while todo:
        n = todo.pop()
        if n in seen or skip.match(n):
            continue
        seen.add(n)
        f = pkgs.get(n)
        if not f:
            print(f"  (not in the index: {n})", flush=True)
            continue
        seen.add(f["Package"])
        out.append(f)
        for dep in f.get("Depends", "").split(","):
            alt = dep.split("|")[0].strip().split(" ")[0].split(":")[0]
            if alt and (alt.startswith("lib") or alt.endswith("-dev") or alt.startswith("gir1.2")):
                todo.append(alt)
    return out

def unpack(fs):
    for f in fs:
        deb = os.path.join(cache, os.path.basename(f["Filename"]))
        if not os.path.exists(deb):
            urllib.request.urlretrieve(f"{mirror}/{f['Filename']}", deb + ".part")
            os.replace(deb + ".part", deb)
        if hashlib.sha256(open(deb, "rb").read()).hexdigest() != f["SHA256"]:
            os.remove(deb)
            sys.exit(f"sha256 mismatch for {f['Filename']} - removed; run again")
        subprocess.run(["dpkg-deb", "-x", deb, root], check=True)

seen = set()
first = closure(want, seen)
print(f"{len(first)} packages for the {arch} sysroot", flush=True)
unpack(first)

# What the declared dependencies miss: a .pc file another one Requires
# (libtiff-4 -> libzstd), or the target of a link (krb5-gssapi.pc ->
# mit-krb5/). Ubuntu's file index names the package owning each; added
# until pkg-config resolves every library Cove links (2026-11-20).
owner = {}
def owners():
    if owner:
        return owner
    cpath = os.path.join(cache, "Contents")
    if not os.path.exists(cpath):
        url = f"{mirror}/dists/{codename}/Contents-{arch}.gz"
        print(f"index {url}", flush=True)
        import gzip
        with urllib.request.urlopen(url, timeout=600) as r:
            open(cpath, "wb").write(gzip.decompress(r.read()))
    for line in open(cpath, encoding="utf-8", errors="replace"):
        path, _, where = line.rstrip("\n").rpartition(" ")
        path = path.strip()
        if "pkgconfig/" in path or path.endswith((".so", ".pc")) or "/mit-krb5/" in path:
            owner[path] = where.split(",")[0].split("/")[-1]
    return owner

triple = {"arm64": "aarch64-linux-gnu", "amd64": "x86_64-linux-gnu"}[arch]
pcdirs = [f"{root}/usr/lib/{triple}/pkgconfig", f"{root}/usr/share/pkgconfig"]
env = dict(os.environ, PKG_CONFIG_SYSROOT_DIR=root, PKG_CONFIG_LIBDIR=":".join(pcdirs))
links = ["webkit2gtk-4.1", "gtk+-3.0", "gdk-3.0", "libsoup-3.0", "javascriptcoregtk-4.1", "glib-2.0", "gio-2.0", "gobject-2.0", "cairo", "pango", "gdk-pixbuf-2.0", "atk"]
for round_ in range(12):
    missing = set()
    for pc in links:
        r = subprocess.run(["pkg-config", "--exists", "--print-errors", pc], env=env, capture_output=True, text=True)
        missing |= set(re.findall(r"Package '?([\w.+-]+)'? was not found", r.stderr))
        missing |= set(re.findall(r"Package '([\w.+-]+)', required by", r.stderr))
    paths = set()
    for d, _, files in os.walk(root):
        for fn in files + _:
            p = os.path.join(d, fn)
            if os.path.islink(p) and not os.path.exists(p):
                paths.add(os.path.relpath(os.path.realpath(p), root))
    if not missing and not paths:
        print(f"pkg-config resolves every library Cove links ({len(links)})", flush=True)
        break
    own = owners()
    want_more = set()
    for m in missing:
        hit = [pkg for path, pkg in own.items() if path.endswith(f"pkgconfig/{m}.pc")]
        want_more |= set(hit[:1]) if hit else set()
        if not hit:
            print(f"  (no package has {m}.pc)", flush=True)
    for pth in paths:
        if pth in own:
            want_more.add(own[pth])
    more = closure(sorted(want_more), seen)
    if not more:
        print(f"still missing, nothing more to add: {sorted(missing)} {sorted(paths)[:5]}", flush=True)
        break
    print(f"round {round_ + 1}: + {', '.join(f['Package'] for f in more)}", flush=True)
    unpack(more)
PY

# Absolute symlinks (libfoo.so -> /usr/lib/...) must point inside the sysroot.
find "$root" -type l | while read -r l; do
  t="$(readlink "$l")"
  case "$t" in /*) ln -sfn "$root$t" "$l" ;; esac
done
echo "✓ $root"
