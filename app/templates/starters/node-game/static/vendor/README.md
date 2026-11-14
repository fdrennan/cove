# Vendored browser libraries · the game starter

Each copied from its npm registry tarball, the tarball checked against the
registry's own sha512 integrity before extraction (React and htm on
2026-10-26, PixiJS on 2026-11-05). Loaded from /static/vendor/ with plain
<script> tags: no npm, no bundler, no network at build or run time.

| file | package | licence |
|---|---|---|
| pixi.min.js | pixi.js 8.22.0 (dist/; its source-map line removed) | MIT, pixi.LICENSE.txt |
| react.production.min.js | react 18.3.1 (umd/) | MIT |
| react-dom.production.min.js | react-dom 18.3.1 (umd/) | MIT |
| htm.umd.js | htm 3.1.1 (dist/) | Apache-2.0 |

sha256 of the files as shipped (build.sh checks them):

```
7a31776e04bd4afde0d4308177d26f377716fcf7e4bd70be590746d6aa594f08  htm.umd.js
2a66ac7373b51d70194dc25073e3011633834c02538965b076cddc9650e2c417  pixi.min.js
35f4f974f4b2bcd44da73963347f8952e341f83909e4498227d4e26b98f66f0d  react-dom.production.min.js
d949f1c3687aedadcedac85261865f29b17cd273997e7f6b2bfc53b2f9d4c4dd  react.production.min.js
```
