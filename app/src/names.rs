//! A domain for every piece of software opened from the stack (operator,
//! 2026-10-17: "when we pull in software it needs a standardized domain name
//! that will work for its paradigm and then a location to reference it ...
//! within cove projects are opened by their domain name").
//!
//!   <registered-name>.<paradigm>.<owned-domain>
//!
//! (The browser build only parses names; allocating them is the server's.)
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]
//!
//! - owned-domain: the root the person owns, by default their personal
//!   `<username>.ndexr.io`; it decides where the project belongs and whose it is.
//! - paradigm: what is being edited, one of PARADIGMS. `prefix` a Gentoo Prefix
//!   package and its packaging, `eb` an EasyBuild build definition, `lang` a
//!   language implementation (R or Python itself), `env` an environment
//!   definition, `r` an R package, `py` a Python distribution; `kernel` and
//!   `src` are reserved for the rest of the stack as their source opens.
//! - registered-name: a readable label for the source's native name.
//!
//! The name is an allocation, not only a function: within one owned root, the
//! same source identity always reopens the project already holding it. That
//! identity (paradigm, authority, native name, subdirectory) is recorded in the
//! project's `.cove-source` with its digest; the hostname is only its address.
//! An identity has exactly four possible names - the readable label, then the
//! label with 12, 16 or 20 characters of its digest - so finding its project is
//! looking in at most four folders, and a name another identity holds is never
//! taken over. Versions are never in the name: they are branches in the repo.

/// The paradigms, in stack order. Only the first seven are opened today
/// (`rs`, Rust crates, since 2026-10-18).
pub const PARADIGMS: [&str; 9] = ["prefix", "eb", "lang", "env", "r", "py", "rs", "kernel", "src"];

/// What a project's source is, exactly: the record its digest is taken over.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub paradigm: &'static str,
    /// Who maintains the namespace the native name lives in.
    pub authority: &'static str,
    /// The name as that authority has it: `sys-devel/gcc`, `BLAST+`,
    /// `data.table`, the PEP 503 name of a Python distribution.
    pub native: String,
    /// The folder inside the authority's source, where one applies.
    pub subdir: String,
}

impl Identity {
    /// The identity of a catalog star (software.rs kinds: prefix, easybuild,
    /// R, Python, Rust). An EasyBuild star is its build definition - opening R's
    /// recipe is `eb`, never `lang`, whose name is kept for when the
    /// interpreter's own source is opened.
    pub fn of(kind: &str, name: &str) -> Option<Identity> {
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        let (paradigm, authority, native) = match kind {
            "prefix" => ("prefix", "gentoo", name.to_owned()),
            "easybuild" => match name.strip_prefix("env/") {
                Some(set) if !set.is_empty() => ("env", "ndexr-manifests", set.to_owned()),
                _ => ("eb", "ndexr-easyconfigs", name.to_owned()),
            },
            // CRAN and Bioconductor share one package namespace (CRAN refuses
            // a name Bioconductor has); a package from elsewhere is not yet
            // told apart by the catalog.
            "R" => ("r", "cran-bioconductor", name.to_owned()),
            "Python" => ("py", "pypi", pep503(name)),
            // crates.io: one namespace, case-blind, `-` and `_` alike.
            "Rust" => ("rs", "crates-io", crate::manifest::crate_norm(name)),
            _ => return None,
        };
        Some(Identity { paradigm, authority, native, subdir: String::new() })
    }

    /// The canonical record: versioned, one field per line.
    pub fn canonical(&self) -> String {
        format!("cove-source-identity/v1\nparadigm={}\nauthority={}\nnative={}\nsubdir={}\n", self.paradigm, self.authority, self.native, self.subdir)
    }

    /// sha256 of the canonical record, as hex.
    pub fn digest(&self) -> String {
        sha256(self.canonical().as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Every name this identity may hold under `owned`, in the order they are
    /// tried: the readable label, then with ever longer digests.
    pub fn hosts(&self, owned: &str) -> Vec<String> {
        let (label, fits) = readable(self.paradigm, &self.native);
        let sfx = base32(&sha256(self.canonical().as_bytes()));
        let mut out = Vec::new();
        if fits {
            out.push(format!("{label}.{}.{owned}", self.paradigm));
        }
        for n in [12usize, 16, 20] {
            let keep = 63 - 1 - n;
            let head = label.get(..keep.min(label.len())).unwrap_or(&label).trim_end_matches('-');
            out.push(format!("{head}-{}.{}.{owned}", &sfx[..n], self.paradigm));
        }
        out
    }
}

/// A Python distribution's name as PyPI compares names: lowercase, every run
/// of `.`, `_` and `-` one hyphen.
pub fn pep503(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if matches!(c, '.' | '_' | '-') {
            if !out.ends_with('-') {
                out.push('-');
            }
        } else {
            out.extend(c.to_lowercase());
        }
    }
    out
}

/// The readable label for a native name, and whether it fits in one DNS label
/// (63) as it is. `sys-devel/gcc` → `gcc-sys-devel`; `BLAST+` → `blast-plus`;
/// anything not a-z0-9 a hyphen, runs collapsed, ends trimmed - so never `--`,
/// never the reserved `xn--` shape.
pub fn readable(paradigm: &str, native: &str) -> (String, bool) {
    let base = match (paradigm, native.split_once('/')) {
        ("prefix", Some((cat, pkg))) => format!("{pkg}-{cat}"),
        ("py", _) => pep503(native),
        _ => native.to_owned(),
    };
    let mut out = String::new();
    for c in base.chars() {
        let piece = match c {
            '+' => "-plus-".to_owned(),
            c if c.is_ascii_alphanumeric() => c.to_ascii_lowercase().to_string(),
            _ => "-".to_owned(),
        };
        for p in piece.chars() {
            if p == '-' && (out.is_empty() || out.ends_with('-')) {
                continue;
            }
            out.push(p);
        }
    }
    let mut out = out.trim_end_matches('-').to_owned();
    if out.is_empty() {
        out.push('x');
    }
    let fits = out.len() <= 63;
    (out, fits)
}

/// A host's parts when it is a software project's: (label, paradigm, owned).
pub fn parse(host: &str) -> Option<(&str, &str, &str)> {
    let (label, rest) = host.split_once('.')?;
    let (paradigm, owned) = rest.split_once('.')?;
    (PARADIGMS.contains(&paradigm) && owned.contains('.') && !label.is_empty()).then_some((label, paradigm, owned))
}

/// Lowercase RFC 4648 base32, no padding.
fn base32(b: &[u8]) -> String {
    const A: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
    let (mut out, mut acc, mut bits) = (String::new(), 0u32, 0u32);
    for &x in b {
        acc = (acc << 8) | x as u32;
        bits += 8;
        while bits >= 5 {
            out.push(A[((acc >> (bits - 5)) & 31) as usize] as char);
            bits -= 5;
        }
    }
    if bits > 0 {
        out.push(A[((acc << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// SHA-256 (FIPS 180-4): small enough to keep here, so names are the same in
/// the browser, the desktop and the server without another crate.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    let mut out = [0u8; 32];
    for i in 0..8 {
        out[4 * i..4 * i + 4].copy_from_slice(&h[i].to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_the_standard() {
        let hex = |b: [u8; 32]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        assert_eq!(hex(sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(hex(sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(base32(b"foobar"), "mzxw6ytboi");
    }

    #[test]
    fn awkward_names_read_as_labels() {
        let l = |p: &str, n: &str| readable(p, n).0;
        assert_eq!(l("prefix", "sys-devel/gcc"), "gcc-sys-devel");
        assert_eq!(l("eb", "BLAST+"), "blast-plus");
        assert_eq!(l("eb", "Boost.Python-NumPy"), "boost-python-numpy");
        assert_eq!(l("eb", "SuperLU_DIST"), "superlu-dist");
        assert_eq!(l("eb", "Xerces-C++"), "xerces-c-plus-plus");
        assert_eq!(l("r", "data.table"), "data-table");
        assert_eq!(l("py", "PyYAML"), "pyyaml");
        assert_eq!(l("py", "cdisc_rules__engine"), "cdisc-rules-engine");
        assert_eq!(l("env", "r4.6_py3.13-release"), "r4-6-py3-13-release");
        assert_eq!(l("eb", "--"), "x");
    }

    #[test]
    fn a_star_names_its_paradigm() {
        let h = |k: &str, n: &str| Identity::of(k, n).unwrap().hosts("fdrennan.ndexr.io").remove(0);
        assert_eq!(h("R", "data.table"), "data-table.r.fdrennan.ndexr.io");
        assert_eq!(h("Python", "PyYAML"), "pyyaml.py.fdrennan.ndexr.io");
        assert_eq!(h("easybuild", "BLAST+"), "blast-plus.eb.fdrennan.ndexr.io");
        assert_eq!(h("easybuild", "env/r4.6_py3.13-release"), "r4-6-py3-13-release.env.fdrennan.ndexr.io");
        assert_eq!(h("prefix", "sys-devel/gcc"), "gcc-sys-devel.prefix.fdrennan.ndexr.io");
        assert!(Identity::of("kernel", "linux").is_none());
        // PyPI's own folding: two spellings are one distribution, one name.
        assert_eq!(Identity::of("Python", "Py_YAML").unwrap().digest(), Identity::of("Python", "py.yaml").unwrap().digest());
        assert_eq!(parse("data-table.r.fdrennan.ndexr.io"), Some(("data-table", "r", "fdrennan.ndexr.io")));
        assert_eq!(parse("cove.ndexr.io"), None);
    }

    #[test]
    fn a_long_or_taken_name_carries_its_digest() {
        let id = Identity::of("R", &"a".repeat(70)).unwrap();
        let hosts = id.hosts("u.ndexr.io");
        // Too long to stand alone: only the digest forms, each label ≤ 63.
        assert_eq!(hosts.len(), 3);
        for h in &hosts {
            assert!(h.split('.').all(|l| l.len() <= 63 && !l.starts_with('-') && !l.ends_with('-')), "{h}");
        }
        assert!(hosts[0].starts_with(&format!("{}-", "a".repeat(50))));
        // Two identities that read alike part at the digest.
        let a = Identity::of("easybuild", "Boost.Python").unwrap().hosts("u.ndexr.io");
        let b = Identity::of("easybuild", "Boost-Python").unwrap().hosts("u.ndexr.io");
        assert_eq!(a[0], b[0]);
        assert_ne!(a[1], b[1]);
        assert_eq!(a[1], Identity::of("easybuild", "Boost.Python").unwrap().hosts("u.ndexr.io")[1], "stable");
    }
}
