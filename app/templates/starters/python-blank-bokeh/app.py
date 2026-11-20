"""__HOST__: a blank Bokeh server app. `bokeh serve --dev` reloads it on every save."""
import sys
from pathlib import Path

import numpy as np
from bokeh.layouts import column
from bokeh.models import ColumnDataSource, Div, Slider
from bokeh.plotting import curdoc, figure

sys.path.insert(0, str(Path(__file__).parent))
from log import log  # noqa: E402

# bokeh serve runs this file once per browser session. run.sh logs the start.
log.info("session opened")

x = np.linspace(0, 4 * np.pi, 400)
source = ColumnDataSource(data={"x": x, "y": np.sin(x)})

plot = figure(title="sin(f · x)", height=320, sizing_mode="stretch_width")
plot.line("x", "y", source=source, line_width=2)

freq = Slider(title="Frequency", start=0.1, end=5, step=0.1, value=1)


def update(attr, old, new):
    source.data = {"x": x, "y": np.sin(new * x)}


freq.on_change("value", update)

doc = curdoc()
doc.title = "__HOST__"
doc.add_root(
    column(
        Div(text="<h1>__HOST__</h1><p>A blank Bokeh app, running. Save a file and it shows here.</p>"),
        freq,
        plot,
        sizing_mode="stretch_width",
        max_width=768,
    )
)
