// The entry module: mount App into #app and keep it mounted. Only platform
// Layers are provided here; each component provides its own domain Layers.
import { BrowserHost } from "@effect/ui/html"
import { Effect, Layer } from "effect"
import { FetchHttpClient } from "effect/http"
import { App } from "./App.js"

const Main = BrowserHost.mount(App, {}, document.getElementById("app")!).pipe(Layer.provide(FetchHttpClient.layer))

Layer.launch(Main).pipe(Effect.runFork)
