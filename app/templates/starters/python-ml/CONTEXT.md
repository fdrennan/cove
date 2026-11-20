# ML playground · __HOST__
A Python app, started from Cove's template: scikit-learn on generated 2-D
datasets (moons, circles, blobs, spiral, XOR). Pick a model - logistic
regression, k-nearest neighbours, an RBF SVM, a random forest or a neural
network - and its parameters; the server fits it on a stratified split and
answers with train and test accuracy and the decision surface, drawn by
matplotlib as a PNG. Nothing is fitted until Train. Change first: `DATASETS`
in app.py - add an entry whose `make` returns your own (X, y) and the page
offers it.

## Files
- app.py - FastAPI, the menu and the work: MODELS and DATASETS (add one and the
  page shows it with its sliders), GET /api/menu, POST /api/train
  (`{"dataset","model","params","n","noise","test","seed"}` -> train_acc,
  test_acc, fit_ms, png)
- static/index.html, static/style.css, static/app.js - the page: the surface
  fills the window, the controls sit in a glass panel built from /api/menu
- run.sh - loads Python from the ndexr stack and serves with uvicorn
- app.env - language, kind (port), port and health path, read by the verbs
- build.sh, up.sh, down.sh, env.sh - byte-compile and import-check app.py;
  start in the background and wait for /health (pid and log in .run/); stop;
  load Python from the ndexr stack

## Run
`./run.sh` serves on 127.0.0.1:$APP_PORT, the port app.env gives this
project (`PORT=<n> ./run.sh` overrides it for one run); `./up.sh` and
`./down.sh` start and stop it in the background.
Python, fastapi, uvicorn, numpy, scikit-learn and matplotlib come from the
stack's env set (env/Python/3.13.15, else env/r4.6_py3.13-release); nothing is
pip-installed.

Logging: log.py (from log import log as applog, since `log` names a log-scale option here), one format across languages; LOG_LEVEL=DEBUG for more.

## Built on
Pedregosa, F. et al. (2011), "Scikit-learn: Machine Learning in Python",
J. Machine Learning Research 12.

## Auth
You and the people you share it with (Project › Access sets who; a new site is never public). The server listens on 127.0.0.1 only; nginx in front decides who
reaches it.

## Open
- (none logged)
