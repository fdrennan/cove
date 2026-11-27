// The saved observations: a Query the runtime keeps beside the Model, and a
// form that saves one through a Command. The Model holds only what the user
// typed; the list itself is never copied into it.
import { Command, Component, UiSchema } from "@effect/ui"
import { Html } from "@effect/ui/html"
import { Effect, Layer, Schema } from "effect"
import { AsyncResult } from "effect/reactivity"
import { ListObservations, Observations, ObservationsTag } from "./domain.js"

export const LogMessage = UiSchema.TaggedEnum({
  UpdatedValue: { value: Schema.String },
  UpdatedNote: { note: Schema.String },
  SubmittedForm: {}
})

// Saves one observation. It reports nothing (no `success`): the query it
// invalidates fetches again and the list shows the new row. App runs it too.
export class AddObservation extends Command.Service<AddObservation>()(
  "observations/AddObservation",
  { args: { value: Schema.Number, note: Schema.String }, invalidates: () => [ObservationsTag] }
) {
  static readonly layer = this.toLayer(
    Effect.gen(function*() {
      const observations = yield* Observations
      return (entry) => observations.add(entry)
    })
  )
}

const LogNoDeps = Html.component({
  name: "Log",
  model: Schema.Struct({ value: Schema.String, note: Schema.String }),
  message: LogMessage,
  queries: { observations: { query: ListObservations, args: () => ({}) } }
}).make({
  init: () => ({ value: "", note: "" }),
  update: {
    UpdatedValue: ($, { value }) => $.evolve({ value }),
    UpdatedNote: ($, { note }) => $.evolve({ note }),
    SubmittedForm: ($) => {
      const value = Number($.model.value)
      return $.model.value.trim() === "" || !Number.isFinite(value)
        ? $.model
        : $.evolve({ value: "", note: "" }).run(AddObservation.make({ value, note: $.model.note.trim() }))
    }
  },
  view: (h, model) =>
    h.section.class("card log").children(
      h.header(h.h2("Observations")),
      h.form.class("row").on("submit", LogMessage.SubmittedForm()).children(
        h.input.attrs({ inputmode: "decimal", placeholder: "value", "aria-label": "Value" }).value(model.value)
          .on("input", ({ value }) => LogMessage.UpdatedValue({ value })),
        h.input.attrs({ placeholder: "note (optional)", "aria-label": "Note" }).class("grow").value(model.note)
          .on("input", ({ value }) => LogMessage.UpdatedNote({ note: value })),
        h.button.class("primary").attr("type", "submit").children(h.isRunning(AddObservation) ? "Saving…" : "Add")
      ),
      AsyncResult.match(h.queries.observations, {
        onInitial: () => h.p.class("quiet").children("Loading…"),
        onFailure: () => h.p.class("quiet").children("The observations could not be loaded."),
        onSuccess: ({ value: rows }) =>
          rows.length === 0
            ? h.p.class("quiet").children("Nothing recorded yet. Press Record, or add one above.")
            : h.table(
              h.thead(h.tr(h.th("Time"), h.th.class("num").children("Value"), h.th("Note"))),
              h.tbody(rows.map((row) =>
                h.tr.key(row.id).children(
                  h.td(new Date(row.at).toLocaleTimeString()),
                  h.td.class("num").children(row.value.toFixed(1)),
                  h.td(row.note)
                )
              ))
            )
      })
    )
})

// The Log provides its own domain Layers; only HttpClient is left for main.ts.
export const Log = LogNoDeps.pipe(
  Component.provide(Layer.mergeAll(ListObservations.layer, AddObservation.layer).pipe(Layer.provide(Observations.layerHttp)))
)
export const AddObservationLayer = AddObservation.layer.pipe(Layer.provide(Observations.layerHttp))
