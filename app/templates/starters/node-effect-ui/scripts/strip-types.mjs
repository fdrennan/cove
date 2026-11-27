// TypeScript to the browser's JavaScript with Node's own type stripping
// (node:module stripTypeScriptTypes, Node 22.13 and later): src/**/*.ts ->
// static/js/**/*.js. Types are removed, never checked - there is no tsc in the
// ndexr stack - and nothing else changes, so a stack trace's line and column
// point at the .ts as written. Only TypeScript that erases is allowed: no
// enum, namespace or constructor parameter properties.
//
//   node scripts/strip-types.mjs [src] [static/js]
import { stripTypeScriptTypes } from 'node:module';
import { mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';

const [src = 'src', out = 'static/js'] = process.argv.slice(2);
rmSync(out, { recursive: true, force: true });
let n = 0;
for (const e of readdirSync(src, { withFileTypes: true, recursive: true })) {
  if (!e.isFile() || !e.name.endsWith('.ts') || e.name.endsWith('.d.ts')) continue;
  const from = join(e.parentPath, e.name);
  const to = join(out, relative(src, from)).replace(/\.ts$/, '.js');
  mkdirSync(dirname(to), { recursive: true });
  try {
    writeFileSync(to, stripTypeScriptTypes(readFileSync(from, 'utf8'), { mode: 'strip', sourceUrl: from }));
  } catch (err) {
    console.error(`${from}: ${err.message}`);
    process.exit(1);
  }
  n += 1;
}
console.log(`strip-types: ${n} files, ${src} -> ${out}`);
