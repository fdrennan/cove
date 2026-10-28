# Vendored browser libraries · shared by every Node starter

Copied from the npm registry tarballs on 2026-10-26, each tarball checked
against the registry's own sha512 integrity before extraction. A starter loads
them from /static/vendor/ with plain <script> tags (never /vendor/: nginx's shared
static_dirs.conf claims that path on every site for the platform's own files): no npm, no bundler, no network at
build or run time, the same rule every other starter keeps.

| file | package | licence |
|---|---|---|
| react.production.min.js | react 18.3.1 (umd/) | MIT |
| react-dom.production.min.js | react-dom 18.3.1 (umd/) | MIT |
| htm.umd.js | htm 3.1.1 (dist/) | Apache-2.0 |

htm gives JSX-like tagged templates (html`<${App} />`) that run as written in
the browser, so a React starter needs no compile step.

sha256 of the files as shipped:

```
7a31776e04bd4afde0d4308177d26f377716fcf7e4bd70be590746d6aa594f08  htm.umd.js
35f4f974f4b2bcd44da73963347f8952e341f83909e4498227d4e26b98f66f0d  react-dom.production.min.js
d949f1c3687aedadcedac85261865f29b17cd273997e7f6b2bfc53b2f9d4c4dd  react.production.min.js
```
