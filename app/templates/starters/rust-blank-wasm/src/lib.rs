//! The part of __HOST__ that runs in the browser, compiled to WebAssembly with no
//! wasm-bindgen: build-web.sh writes dist/web/app.wasm and static/app.js calls these.

#[no_mangle]
pub extern "C" fn add(a: f64, b: f64) -> f64 {
    a + b
}
