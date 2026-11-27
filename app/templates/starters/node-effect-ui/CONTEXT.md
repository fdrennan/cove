# __HOST__ · an Effect UI app, TypeScript

Effect UI (https://ui.effect.website, github.com/Effect-TS/ui) is a frontend
library on Effect and the Elm Architecture: each component is Schemas for its
props, Model, messages and events, plus pure `init`, `update` and `view`.
Side effects are values - Commands and Subscriptions the runtime runs - and
data comes from Queries. It is in public preview; read `docs/effect-ui.md`
(the library's own guide for agents, from the pinned snapshot) before changing
a component, and follow its naming.

## How it fits together

| file | what it is |
|---|---|
| `src/main.ts` | mounts `App` into `#app`; provides only platform Layers (`FetchHttpClient`) |
| `src/App.ts` | the page: `Meter` and `Log` side by side; hears the Meter's `Recorded` event as `RecordedMeter` and saves it |
| `src/Meter.ts` | a live reading: a clock Subscription samples every 200 ms while it runs, drawn as an SVG trace |
| `src/Log.ts` | saved observations: the `ListObservations` Query, and `AddObservation`, the Command that saves one |
| `src/domain.ts` | the `Observation` Schema, the `Observations` service (all HTTP), the Query and its tag |
| `server.js` | serves `static/`, `/health` and the JSON API; Node standard library only |
| `data/observations.json` | what was saved (newest first, at most 500) |

The rules the example follows, from the guide:
- A message is a fact, past tense: `ClickedRecord`, `Sampled`, never `Save`.
- A parent cannot message a child: it passes props down and maps the child's
  `events` in `onEmit` (`Recorded` -> `RecordedMeter`).
- The Model holds what the user did. Fetched data stays in the Query's
  result (`h.queries.observations`), never copied into the Model.
- I/O goes through a service that Commands and Queries capture; `update` and
  `view` stay pure. A Command that changes data `invalidates` its Query's tag.
- A Subscription is "while ...": listed only while the Meter runs.

## Building

`.cove/build.sh` turns `src/*.ts` into `static/js/*.js` with the stack's own
Node (`node:module` `stripTypeScriptTypes`), checks every script, and checks
the vendored files' sha256. `.cove/node/run.sh` strips again on every save in
`src/`, and the open page reloads itself. No npm, no bundler, no network.

- **Types are stripped, not checked:** there is no TypeScript compiler in the
  ndexr stack. Write TypeScript that erases (no `enum`, `namespace` or
  constructor parameter properties). Cove checked this example with tsc 7.0.2
  under `strict` and `exactOptionalPropertyTypes`, which Effect UI requires.
- Relative imports end in `.js` (`./Log.js`), the file the page loads.
- `src/` imports the packages by their own names (`effect`,
  `@effect/ui/html`); `static/index.html`'s import map sends each to its file
  in `static/vendor/effect/`. A path not in the map needs a new bundle:
  `static/vendor/effect/README.md` says how it was made.
- Paths are relative (`static/...`, `api/...`), so the page works on its own
  domain and in Cove's preview under `/port/<n>/`.

## Logging
log.js (`const log = require('./log')`) in server.js only, never the browser code in src/; one format across languages; LOG_LEVEL=DEBUG for more.
