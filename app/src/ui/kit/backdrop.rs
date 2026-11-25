//! ndexr.io's look behind the IDE (operator, 2026-10-14: "use the nighttime
//! camping picture as the background ... the image, the windows"): the
//! star-trails photograph - the lit tent under the circling sky, ndexr.io's
//! images/lp/star-trails-1920.webp - edge to edge, darkened the way the site
//! darkens it, with the panes as translucent windows over it (theme.rs `pane`).

use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, Pos2, Rect, TextureHandle, Vec2};

use crate::ui::kit::theme;

const PHOTO: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/backdrops/star-trails-1920.webp"));

/// Decoded off the UI thread at start (a browser has none: there, inline);
/// uploaded to the GPU on first paint.
pub struct Backdrop {
    decoded: Arc<Mutex<Option<egui::ColorImage>>>,
    tex: Option<TextureHandle>,
}

impl Backdrop {
    pub fn start(ctx: &egui::Context) -> Self {
        let decoded = Arc::new(Mutex::new(None));
        let (d, ctx) = (decoded.clone(), ctx.clone());
        let work = move || {
            if let Ok(img) = image::load_from_memory_with_format(PHOTO, image::ImageFormat::WebP) {
                let rgba = img.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                *d.lock().unwrap() = Some(egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()));
                ctx.request_repaint();
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(work);
        #[cfg(target_arch = "wasm32")]
        work();
        Backdrop { decoded, tex: None }
    }

    /// The photograph "cover"-fitted to the window, under everything, with
    /// ndexr.io's darkening laid over it so text on the windows stays legible.
    pub fn paint(&mut self, ctx: &egui::Context) {
        if self.tex.is_none() {
            if let Some(img) = self.decoded.lock().unwrap().take() {
                self.tex = Some(ctx.load_texture("star-trails", img, egui::TextureOptions::LINEAR));
            }
        }
        let screen = ctx.content_rect();
        let p = ctx.layer_painter(egui::LayerId::background());
        p.rect_filled(screen, 0, theme::ground());
        // Light: no night sky - warm paper, a shade deeper toward the bottom,
        // so the windows sit on something calm rather than a washed-out photo.
        if theme::is_light() {
            let mut m = egui::Mesh::default();
            // In the look's own colours: its surface at the top, its ground below.
            let (top, low) = (theme::surface(), theme::ground());
            m.colored_vertex(screen.left_top(), top);
            m.colored_vertex(screen.right_top(), top);
            m.colored_vertex(screen.right_bottom(), low);
            m.colored_vertex(screen.left_bottom(), low);
            m.add_triangle(0, 1, 2);
            m.add_triangle(0, 2, 3);
            p.add(m);
            return;
        }
        if let Some(t) = &self.tex {
            let [w, h] = t.size();
            let (iw, ih) = (w as f32, h as f32);
            let scale = (screen.width() / iw).max(screen.height() / ih);
            let (uw, uh) = (screen.width() / (iw * scale), screen.height() / (ih * scale));
            // Anchored low: the tent and the horizon stay in view on any shape.
            let uv = Rect::from_center_size(Pos2::new(0.5, 1.0 - uh / 2.0), Vec2::new(uw, uh));
            p.image(t.id(), screen, uv, Color32::WHITE);
            p.rect_filled(screen, 0, Color32::from_black_alpha(70));
        }
    }
}

/// The calm photograph the restart notice opens with (operator, 2026-10-15:
/// "a little larger of a modal with another calm photo ... a softer visual
/// other than reading lots of text alone"): ndexr.io's night lake, still
/// water under the stars. Decoded on first use - only a page whose session
/// ended ever needs it.
const CALM: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/backdrops/night-lake-960.webp"));

#[derive(Default)]
pub struct Calm {
    tex: Option<TextureHandle>,
    tried: bool,
}

impl Calm {
    pub fn texture(&mut self, ctx: &egui::Context) -> Option<&TextureHandle> {
        if !self.tried {
            self.tried = true;
            if let Ok(img) = image::load_from_memory_with_format(CALM, image::ImageFormat::WebP) {
                let rgba = img.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let img = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                self.tex = Some(ctx.load_texture("night-lake", img, egui::TextureOptions::LINEAR));
            }
        }
        self.tex.as_ref()
    }
}
