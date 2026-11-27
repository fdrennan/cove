// What the app is about, and the one service all of its I/O goes through.
// Components never name the service: the Query and the Command below capture
// it, so `update` and `view` stay pure.
import { Query } from "@effect/ui"
import { Context, Effect, Layer, Schema } from "effect"
import { HttpClient, HttpClientRequest, HttpClientResponse } from "effect/http"

export const Observation = Schema.Struct({
  id: Schema.String,
  at: Schema.String,
  value: Schema.Number,
  note: Schema.String
})
export type Observation = typeof Observation.Type

// Every query of observations carries this tag. A Command that changes them
// invalidates it, and every query carrying it fetches again.
export const ObservationsTag = Query.Tag("observations")

export class Observations extends Context.Service<Observations, {
  readonly list: Effect.Effect<ReadonlyArray<Observation>>
  add(entry: { readonly value: number; readonly note: string }): Effect.Effect<Observation>
}>()("app/Observations") {
  // The server's JSON API (server.js ROUTES). Only `HttpClient` is left for
  // the entry module to provide: FetchHttpClient in the browser.
  static readonly layerHttp = Layer.effect(
    Observations,
    Effect.gen(function*() {
      const client = (yield* HttpClient.HttpClient).pipe(
        // Beside the page, wherever it is served: its own domain, or Cove's
        // preview at /port/<n>/. Effect resolves a bare "api" from the origin.
        HttpClient.mapRequestInput(HttpClientRequest.prependUrl(new URL("api", document.baseURI).href)),
        HttpClient.filterStatusOk
      )
      const list = client.get("/observations").pipe(
        Effect.flatMap(HttpClientResponse.schemaBodyJson(Schema.Array(Observation))),
        Effect.orDie
      )
      const add = (entry: { readonly value: number; readonly note: string }) =>
        HttpClientRequest.post("/observations").pipe(
          HttpClientRequest.bodyJsonUnsafe(entry),
          client.execute,
          Effect.flatMap(HttpClientResponse.schemaBodyJson(Observation)),
          Effect.orDie
        )
      return Observations.of({ list, add })
    })
  )
}

// The saved observations, newest first. Cached per args (none here), and
// fetched again whenever ObservationsTag is invalidated.
export class ListObservations extends Query.Service<ListObservations>()(
  "observations/ListObservations",
  { success: Schema.Array(Observation), staleTime: "30 seconds", tags: [ObservationsTag] }
) {
  static readonly layer = this.toLayer(
    Effect.gen(function*() {
      const observations = yield* Observations
      return () => observations.list
    })
  )
}
