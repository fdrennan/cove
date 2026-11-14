// __HOST__: loads src/lib.rs, compiled to /app.wasm by ./build.sh. No bundler, no bindgen.
(async () => {
  const { instance } = await WebAssembly.instantiateStreaming(fetch('/app.wasm'));
  document.querySelector('p').textContent = `A blank WebAssembly app, running. Rust says 2 + 2 = ${instance.exports.add(2, 2)}.`;
})();
