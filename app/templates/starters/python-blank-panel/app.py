"""__HOST__: a blank Panel app. `panel serve --dev` reloads it on every save."""
import sys
from pathlib import Path

import numpy as np
import panel as pn

sys.path.insert(0, str(Path(__file__).parent))
from log import log  # noqa: E402

pn.extension(sizing_mode="stretch_width")

# panel serve runs this file once per browser session. run.sh logs the start.
log.info("session opened")

points = pn.widgets.IntSlider(name="Points", start=10, end=1000, step=10, value=200)


def summary(n):
    x = np.random.default_rng(1).normal(size=n)
    return pn.pane.Markdown(f"**{n}** draws: mean {x.mean():.3f}, sd {x.std(ddof=1):.3f}")


pn.Column(
    "# __HOST__",
    "A blank Panel app, running. Save a file and it shows here.",
    points,
    pn.bind(summary, points),
    max_width=768,
).servable(title="__HOST__")
