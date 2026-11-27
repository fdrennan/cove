# Vendored Effect and Effect UI · the Effect UI starter

Effect UI is in public preview and not on npm yet: it is published as a
snapshot per commit at pkg.pr.new. This copy is pinned to one snapshot, so the
app needs no npm, no bundler and no network to build or run.

| package | version | from | licence |
|---|---|---|---|
| effect | 4.1.6 | registry.npmjs.org (sha512-P3CxMsdcSCle/7/FnPILd9GJkNqGtduKam2LX9Xsb8LoOjmeQ9Kmsuh16JwtIthaxLyiTCOzUUt+PZsXIohl+A==) | MIT, effect.LICENSE.txt |
| @effect/ui | main at 9465cfb (2026-11-27) | pkg.pr.new/Effect-TS/ui/@effect/ui@9465cfb (sha512-unJQK4vPAY0MMG8h4VdZ94pMWX0H6bNKqynobgCeUvDR0hg/uoZpKUNkHfsl998g45Lv+Nt+mqUdocqs1vva/A==) | MIT, effect-ui.LICENSE.txt |

Bundled once by Cove (esbuild 0.28.2: `--bundle --splitting --format=esm
--minify --target=es2022`), one entry per import path, each `export *` of that
path; the `chunk-*.js` files are what they share, so the page loads one copy
of Effect. index.html's import map gives each path its file:

| import | file |
|---|---|
| `effect` | effect.js |
| `effect/http` | effect-http.js |
| `effect/reactivity` | effect-reactivity.js |
| `@effect/ui` | effect-ui.js |
| `@effect/ui/html` | effect-ui-html.js |

Another path (`@effect/ui-kit`, `effect/unstable/...`) needs a new bundle with
that entry added, and a line in the import map.

sha256 of the files as shipped (build.sh checks them):

```
d18c4d62fc8b17c9a273610af252a558f06eaf31007cace6ec0af64359fa0c65  chunk-37UNQLKM.js
71b3ab7d0409bc898c604196ed5a3796c7fe2e6dc2022c71c5574a2983d726f4  chunk-F73SPF77.js
3a9655fc059b024d232960fc9e58bee57042fbcea1e373ef0e094435e8cbe35a  chunk-J7QRDKUI.js
afdac9f74478189bdaef8ff669003e9cd75234f39e2daf40a93a846b69f87ae6  chunk-KGCTEZU7.js
fd9806c9b48e1730f19cc342f366097485560977807dc89ed3bf8265dc47219d  chunk-O6VMEBRE.js
9b5481fb26364b7c1205a19c4e9ad2ee5e137e3585c84748608d674fdb747d7d  chunk-W2KUCBFV.js
f78d58f3c17ffeb781992cd8a9d7f881a6ff8b53b5e30d064c9c02f09af1eb1d  chunk-ZQQDSCIO.js
de54c3522ca94f22a15ca1886eebc9478da790d437d13f9fb90d86e15572a577  effect-http.js
a9bd117e4b786c22cc822537232ca5d173fdcef6bed282b0d43be6bfdc6ac9f4  effect-reactivity.js
8a6632ce7f2e4d7a61608c5d51c55133e3747e1cfca67ee2a40d28cd0dab1dae  effect-ui-html.js
bb63bef995e170b1e8c430b1d5b367b8938495c336f52293fcca53f408efa213  effect-ui.js
d885c95f8c834918474a50f4330691d1a15004b4ca752f23c637704a57acb80b  effect.js
```
