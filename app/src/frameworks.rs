//! The marks of the frameworks Cove's projects are built with - Shiny,
//! FastAPI, React, Rust, Julia, ... - beside a project wherever one is picked
//! or open, so a person sees what kind of app they are working on as they move
//! between them (operator, 2026-10-28). The pictures are the projects' own
//! (assets/frameworks/SOURCES.md says where each came from); the key is
//! apps.rs `framework`, carried on api::DomainInfo.

use eframe::egui;

use crate::theme;

pub struct Mark {
    pub key: &'static str,
    /// As a person reads it.
    pub name: &'static str,
    png: &'static [u8],
    /// A one-colour mark drawn white: tinted with the theme's text colour so
    /// it reads on light looks as on dark.
    tint: bool,
}

macro_rules! mark {
    ($key:literal, $name:literal, $tint:literal) => {
        Mark { key: $key, name: $name, png: include_bytes!(concat!("../assets/frameworks/", $key, ".png")), tint: $tint }
    };
}

pub const MARKS: &[Mark] = &[
    mark!("shiny", "Shiny", false),
    mark!("r", "R", false),
    mark!("fastapi", "FastAPI", false),
    mark!("flask", "Flask", true),
    mark!("streamlit", "Streamlit", false),
    mark!("python", "Python", false),
    mark!("react", "React", false),
    mark!("nodejs", "Node.js", false),
    mark!("wasm", "WebAssembly", false),
    mark!("rust", "Rust", true),
    mark!("julia", "Julia", false),
    mark!("cplusplus", "C++", false),
    mark!("typescript", "TypeScript", false),
    mark!("code", "An IDE", true),
];

pub fn mark(key: &str) -> Option<&'static Mark> {
    MARKS.iter().find(|m| m.key == key)
}

/// A starter's mark, from starters.rs `Way`'s language and framework words.
pub fn of_way(lang: &str, framework: &str) -> &'static str {
    match lang {
        "R" if framework.contains("Shiny") => "shiny",
        "R" => "r",
        "Python" if framework.contains("FastAPI") => "fastapi",
        "Python" if framework.contains("Flask") => "flask",
        "Python" if framework.contains("Streamlit") => "streamlit",
        "Python" => "python",
        "React" => "react",
        "Node" => "nodejs",
        "Rust" if framework.contains("WebAssembly") => "wasm",
        "Rust" => "rust",
        "Julia" => "julia",
        "C++" => "cplusplus",
        "TypeScript" => "typescript",
        "VS Code" => "code",
        _ => "",
    }
}

/// The mark as a square of `size` points, its name on hover. Nothing is drawn
/// for a key with no mark. Decoded once per key, on first use.
pub fn icon(ui: &mut egui::Ui, key: &str, size: f32) -> Option<egui::Response> {
    let m = mark(key)?;
    let id = egui::Id::new(("framework-mark", m.key));
    let tex = ui.ctx().data(|d| d.get_temp::<egui::TextureHandle>(id)).or_else(|| {
        let img = image::load_from_memory_with_format(m.png, image::ImageFormat::Png).ok()?.to_rgba8();
        let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
        let t = ui.ctx().load_texture(format!("framework-{}", m.key), ci, egui::TextureOptions::LINEAR);
        ui.ctx().data_mut(|d| d.insert_temp(id, t.clone()));
        Some(t)
    })?;
    let img = egui::Image::new(egui::load::SizedTexture::new(tex.id(), egui::vec2(size, size)))
        .tint(if m.tint { theme::text_strong() } else { egui::Color32::WHITE });
    Some(ui.add(img).on_hover_text(m.name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mark_decodes_and_every_starter_has_one() {
        for m in MARKS {
            let img = image::load_from_memory_with_format(m.png, image::ImageFormat::Png).unwrap_or_else(|e| panic!("{}: {e}", m.key));
            assert_eq!((img.width(), img.height()), (96, 96), "{}", m.key);
        }
        for cat in crate::starters::CATEGORIES {
            for w in cat.ways.iter().filter(|w| w.starter.is_some()) {
                let k = of_way(w.lang, w.framework);
                assert!(mark(k).is_some(), "{} {} has no mark", w.lang, w.framework);
            }
        }
    }
}
