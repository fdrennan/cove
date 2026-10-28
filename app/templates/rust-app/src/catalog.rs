//! The ndexr stack's catalog, read by the server for the atlas: every package
//! the stack builds, in six layers, and what each one needs. The same catalog
//! Cove's atlas draws (cove.ndexr.io/view/software.json, public).
//!
//! Read from, in order: $NDEXR_CATALOG (a file), the platform's own copy
//! (/srv/ndexr/software/catalog.json), then $NDEXR_CATALOG_URL (default the
//! public one). Kept ten minutes, then read again.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::api::{AtlasView, Star};

/// The catalog's layer keys, bottom of the stack first, and how they read.
const LAYERS: &[(&str, &str)] = &[
    ("prefix", "Gentoo Prefix"),
    ("easybuild", "EasyBuild"),
    ("R", "R"),
    ("Python", "Python"),
    ("Julia", "Julia"),
    ("Rust", "Rust"),
];

const PUBLIC: &str = "https://cove.ndexr.io/view/software.json";
const KEEP: Duration = Duration::from_secs(600);

static CACHE: Mutex<Option<(Instant, AtlasView)>> = Mutex::new(None);

pub fn atlas() -> Result<AtlasView, String> {
    if let Some((at, v)) = CACHE.lock().unwrap().as_ref() {
        if at.elapsed() < KEEP {
            return Ok(v.clone());
        }
    }
    let (raw, source) = read()?;
    let v = parse(&raw, source)?;
    *CACHE.lock().unwrap() = Some((Instant::now(), v.clone()));
    Ok(v)
}

fn read() -> Result<(Vec<u8>, String), String> {
    let files = std::env::var("NDEXR_CATALOG").ok().into_iter().chain(["/srv/ndexr/software/catalog.json".to_owned()]);
    for f in files {
        if let Ok(b) = std::fs::read(&f) {
            return Ok((b, f));
        }
    }
    let url = std::env::var("NDEXR_CATALOG_URL").unwrap_or_else(|_| PUBLIC.to_owned());
    match ehttp::fetch_blocking(&ehttp::Request::get(&url)) {
        Ok(r) if r.ok => Ok((r.bytes, url)),
        Ok(r) => Err(format!("{url}: {} {}", r.status, r.status_text)),
        Err(e) => Err(format!("{url}: {e}")),
    }
}

/// The catalog's items as stars. Items keep their order, so the catalog's
/// own dependency indices still point at the right star.
pub fn parse(raw: &[u8], source: String) -> Result<AtlasView, String> {
    let j: Value = serde_json::from_slice(raw).map_err(|e| format!("{source}: {e}"))?;
    let items = j["items"].as_array().ok_or_else(|| format!("{source}: no items"))?;
    let s = |v: &Value| v.as_str().unwrap_or_default().to_owned();
    let stars = items
        .iter()
        .map(|it| Star {
            name: s(&it["n"]),
            layer: LAYERS.iter().position(|(k, _)| *k == it["k"].as_str().unwrap_or_default()).unwrap_or(0) as u8,
            group: s(&it["g"]),
            purpose: s(&it["p"]),
            archs: it["a"].as_array().map(|a| a.iter().map(s).collect()).unwrap_or_default(),
            ok: it["st"]["ok"].as_u64().unwrap_or(0) as u32,
            failed: it["st"]["fail"].as_u64().unwrap_or(0) as u32,
            deps: it["dep"].as_array().map(|d| d.iter().filter_map(|x| x.as_u64()).map(|x| x as u32).filter(|x| (*x as usize) < items.len()).collect()).unwrap_or_default(),
        })
        .collect();
    Ok(AtlasView { layers: LAYERS.iter().map(|(_, n)| n.to_string()).collect(), stars, source, generated: j["generated"].as_u64().unwrap_or(0) })
}

#[cfg(test)]
mod tests {
    #[test]
    fn items_become_stars_with_their_layer_and_needs() {
        let raw = br#"{"generated": 5, "items": [
            {"n": "sys-libs/zlib", "k": "prefix", "g": "sys-libs", "p": "compression", "a": ["x86_64"], "st": {"ok": 3, "fail": 1}},
            {"n": "data.table", "k": "R", "g": "Data", "p": "fast tables", "dep": [0, 9]}
        ]}"#;
        let v = super::parse(raw, "test".into()).unwrap();
        assert_eq!(v.layers[0], "Gentoo Prefix");
        assert_eq!((v.stars[0].layer, v.stars[0].ok, v.stars[0].failed), (0, 3, 1));
        assert_eq!(v.stars[1].layer, 2);
        // An index past the end is dropped, not drawn to nowhere.
        assert_eq!(v.stars[1].deps, vec![0]);
        assert_eq!(v.generated, 5);
    }
}
