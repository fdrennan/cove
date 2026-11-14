"""__HOST__: a blank Dash app. The page reloads itself on every save."""
import os

import plotly.express as px
from dash import Dash, Input, Output, dcc, html

app = Dash(__name__, title="__HOST__")
server = app.server

app.layout = html.Main(
    [
        html.H1("__HOST__"),
        html.P("A blank Dash app, running. Save a file and it shows here."),
        html.Label("Points", htmlFor="points"),
        dcc.Slider(10, 500, 10, value=100, id="points", marks=None, tooltip={"placement": "bottom"}),
        dcc.Graph(id="chart"),
    ],
    style={"maxWidth": "48rem", "margin": "2rem auto", "fontFamily": "system-ui, sans-serif"},
)


@app.callback(Output("chart", "figure"), Input("points", "value"))
def chart(n):
    import numpy as np

    x = np.random.default_rng(1).normal(size=n)
    return px.histogram(x=x, nbins=30, title=f"{n} draws from a normal distribution")


@server.get("/health")
def health():
    return {"ok": True}


if __name__ == "__main__":
    # debug=False: Flask's debugger runs code typed in the browser, and nginx
    # puts this on the web. Hot reload (the page) and the reloader (the
    # server) are what a save needs, without it.
    app.run(
        host="127.0.0.1",
        port=int(os.environ["PORT"]),
        debug=False,
        dev_tools_hot_reload=True,
        use_reloader=True,
        # The "stat" reloader checks the files the app imported; watchdog (the
        # default, in the stack) watches every folder of the site library:
        # 14,474 inotify watches for one blank app (2026-11-07).
        reloader_type="stat",
    )
