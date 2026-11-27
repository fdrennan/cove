// The page: the Meter and the Log side by side. A parent never sends a child a
// message; it hears the child's events in `onEmit`. The Meter's Recorded
// becomes RecordedMeter here, which saves the reading.
import { Component, UiSchema } from "@effect/ui"
import { Html } from "@effect/ui/html"
import { Schema } from "effect"
import { AddObservation, AddObservationLayer, Log } from "./Log.js"
import { Meter } from "./Meter.js"

export const AppMessage = UiSchema.TaggedEnum({ RecordedMeter: { value: Schema.Number } })

const AppNoDeps = Html.component({ name: "App", model: Schema.Struct({ recorded: Schema.Number }), message: AppMessage }).make({
  init: () => ({ recorded: 0 }),
  update: {
    RecordedMeter: ($, { value }) =>
      $.evolve({ recorded: (n) => n + 1 }).run(AddObservation.make({ value, note: "from the meter" }))
  },
  view: (h, model) =>
    h.main(
      h.header.class("top").children(
        h.h1("__HOST__"),
        h.p.class("lead").children("Effect UI: components as Schemas and pure functions, effects as values.")
      ),
      h.div.class("grid").children(
        Meter(h, { onEmit: (event) => AppMessage.RecordedMeter({ value: event.value }) }),
        Log(h, {})
      ),
      h.footer.children(`${model.recorded} recorded from the meter this visit`)
    )
})

export const App = AppNoDeps.pipe(Component.provide(AddObservationLayer))
