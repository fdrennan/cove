// A live instrument: while it runs, a clock Subscription samples a reading
// every 200 ms and the view draws the last 60 as a trace. Record reports the
// latest reading to the parent as an event; the Meter does not save anything.
import { Subscription, UiSchema } from "@effect/ui"
import { Html } from "@effect/ui/html"
import { Clock, Effect, Random, Schema, Stream } from "effect"

const WINDOW = 60

export const MeterMessage = UiSchema.TaggedEnum({
  ClickedStart: {},
  ClickedPause: {},
  ClickedReset: {},
  ClickedRecord: {},
  Sampled: { value: Schema.Number }
})

export const MeterEvents = UiSchema.TaggedEnum({ Recorded: { value: Schema.Number } })

// A slow wave with noise on it: something worth watching. The time comes from
// the clock service, so a test can drive it.
const sample = Effect.gen(function*() {
  const t = (yield* Clock.currentTimeMillis) / 1000
  const noise = yield* Random.next
  return Math.round((50 + 30 * Math.sin(t / 2) + 8 * noise) * 10) / 10
})

// The trace as an SVG path, 0-100 scaled into the box.
const trace = (readings: ReadonlyArray<number>, w: number, h: number) =>
  readings.map((v, i) => `${i === 0 ? "M" : "L"}${(i / (WINDOW - 1)) * w},${h - (v / 100) * h}`).join(" ")

export const Meter = Html.component({
  name: "Meter",
  model: Schema.Struct({ running: Schema.Boolean, readings: Schema.Array(Schema.Number) }),
  message: MeterMessage,
  events: MeterEvents
}).make({
  init: () => ({ running: true, readings: [] }),
  update: {
    ClickedStart: ($) => $.evolve({ running: true }),
    ClickedPause: ($) => $.evolve({ running: false }),
    ClickedReset: ($) => $.evolve({ readings: [] }),
    ClickedRecord: ($) => {
      const last = $.model.readings.at(-1)
      return last === undefined ? $.model : $.evolve({}).emit(MeterEvents.Recorded({ value: last }))
    },
    Sampled: ($, { value }) => $.evolve({ readings: (r) => [...r, value].slice(-WINDOW) })
  },
  // Listed while running, so pausing stops the stream and starting begins a
  // new one: a Subscription is "while ...", a Command is "once".
  subscriptions: ({ model }) => [
    model.running &&
    Subscription.make("sample", "on", () =>
      Stream.tick("200 millis").pipe(
        Stream.mapEffect(() => sample),
        Stream.map((value) => MeterMessage.Sampled({ value }))
      ))
  ],
  view: (h, model) => {
    const last = model.readings.at(-1)
    return h.section.class("card meter").children(
      h.header(
        h.h2("Live reading"),
        h.span.class("pill", model.running ? "on" : "off").children(model.running ? "sampling" : "paused")
      ),
      h.p.class("big").children(last === undefined ? "–" : last.toFixed(1)),
      h.svg.attrs({ viewBox: "0 0 600 140", class: "trace", "aria-label": "the last 60 readings" }).children(
        h.path.attrs({ d: trace(model.readings, 600, 140) })
      ),
      h.div.class("row").children(
        model.running
          ? h.button.on("click", MeterMessage.ClickedPause()).children("Pause")
          : h.button.on("click", MeterMessage.ClickedStart()).children("Start"),
        h.button.on("click", MeterMessage.ClickedReset()).children("Reset"),
        h.button.class("primary").disabled(last === undefined).on("click", MeterMessage.ClickedRecord()).children("Record")
      )
    )
  }
})
