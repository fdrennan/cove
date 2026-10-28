//! Records what this binary was built from, for GET /api/env: the compiler,
//! the target, the profile, the stack modules loaded at build, and the
//! version Cargo.lock resolved for each crate in [dependencies]. Read from the
//! build itself, so the page can never show a version that was typed.

use std::process::Command;

fn main() {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let version = Command::new(&rustc)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map_or_else(|| "unknown".into(), |s| s.trim().to_owned());
    println!("cargo:rustc-env=NETWATCH_RUSTC={version}");
    println!("cargo:rustc-env=NETWATCH_TARGET={}", std::env::var("TARGET").unwrap_or_default());
    println!("cargo:rustc-env=NETWATCH_PROFILE={}", std::env::var("PROFILE").unwrap_or_default());
    println!("cargo:rustc-env=NETWATCH_MODULES={}", std::env::var("LOADEDMODULES").unwrap_or_default());

    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    let toml = std::fs::read_to_string(format!("{dir}/Cargo.toml")).unwrap_or_default();
    let lock = std::fs::read_to_string(format!("{dir}/Cargo.lock")).unwrap_or_default();
    // Direct dependencies: the `name = ...` lines under [dependencies].
    let mut deps = Vec::new();
    let mut inside = false;
    for line in toml.lines().map(str::trim) {
        if line.starts_with('[') {
            inside = line == "[dependencies]";
        } else if inside && !line.is_empty() && !line.starts_with('#') {
            if let Some((name, _)) = line.split_once('=') {
                deps.push(name.trim().to_owned());
            }
        }
    }
    // Their resolved versions: Cargo.lock's [[package]] blocks.
    let mut found = Vec::new();
    for block in lock.split("[[package]]") {
        let field = |k: &str| {
            block.lines().find_map(|l| l.trim().strip_prefix(k).map(|v| v.trim_start_matches([' ', '=']).trim_matches('"').to_owned()))
        };
        if let (Some(n), Some(v)) = (field("name ="), field("version =")) {
            if deps.contains(&n) {
                found.push(format!("{n}={v}"));
            }
        }
    }
    println!("cargo:rustc-env=NETWATCH_DEPS={}", found.join(","));
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=Cargo.lock");
    println!("cargo:rerun-if-env-changed=LOADEDMODULES");
}
