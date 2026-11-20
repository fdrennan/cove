# Digit Lab · __HOST__
A Python app, started from Cove's template: draw a digit with a mouse or a
finger and three scikit-learn classifiers - an RBF support vector machine,
logistic regression and k-nearest neighbours - read it as you draw, each with
its probabilities for 0-9. They are fitted at startup (about two seconds) on
scikit-learn's built-in handwritten digits, 1,797 8x8 images, no download; the
page shows each model's train and held-out accuracy, its confusion matrix, and
the 8x8 image it classified. Sample draws a held-out digit onto the pad. It
shows what Python is good at: numpy for the image work and scikit-learn's
uniform fit/predict_proba interface, so three model families sit behind one
loop. A starting point: describe what this site is for here.

## Files
- app.py - FastAPI and the work. MODELS is the menu (add an entry and it is
  trained, scored and shown); train_all() fits on a stratified 75/25 split;
  to_8x8() turns a drawing into a dataset-style image.
  GET /api/models -> per model `label, train_acc, test_acc, fit_ms, confusion`
  (10x10, true digit by row); GET /api/sample[?i=] -> a held-out
  `{i, label, pixels[64]}`; POST /api/predict `{"ink": [4096 ints 0-255]}`
  (64x64, row-major) -> `seen` (the 8x8, 0-16), per model `digit, probs[10]`, `ms`
- static/index.html, static/style.css, static/app.js - the page: the pad in the
  middle, models and confusion matrix on the left, the verdict, probability
  bars and the 8x8 on the right; on a phone they stack under the pad
- run.sh - loads Python from the ndexr stack and serves with uvicorn

## Run
`./run.sh` serves on 127.0.0.1:$PORT (default APP_PORT in app.env):
`PORT=8810 ./run.sh`. Python, fastapi, uvicorn, numpy and scikit-learn come
from the stack's env set (env/Python/3.13.15, else env/r4.6_py3.13-release);
nothing is pip-installed. app.py pins BLAS/OpenMP to one thread: with one
thread per core, logistic regression on this box took 78 s instead of 0.2 s.

Logging: log.py (from log import log), one format across languages; LOG_LEVEL=DEBUG for more.

## Built on
- The data: UCI Optical Recognition of Handwritten Digits (Alpaydin & Kaynak,
  1998; C. Kaynak, MSc thesis, Bogazici University, 1995), shipped in
  scikit-learn as `load_digits`. Each scanned digit was normalised to a 32x32
  bitmap and its on-pixels counted in 4x4 blocks, giving 8x8 values of 0-16.
  to_8x8() repeats that: crop to the ink, scale to 32 rows keeping the aspect
  (every image in the set fills all 8 rows), centre the columns on the centre
  of mass, threshold, count. Checked: held-out digits blurred up to 48x48 and
  put back through to_8x8() are still read correctly 96% of the time by the SVM.
- SVM: Cortes & Vapnik, "Support-vector networks", Machine Learning 20 (1995);
  probabilities by Platt scaling (Platt, 1999) through CalibratedClassifierCV.
- Logistic regression (multinomial, L-BFGS) and k-NN: Cover & Hart, "Nearest
  neighbor pattern classification", IEEE Trans. Inf. Theory 13 (1967).
- scikit-learn: Pedregosa et al., JMLR 12 (2011).

## Auth
You and the people you share it with (Project › Access sets who; a new site is never public). The server listens on 127.0.0.1 only; nginx in front decides who
reaches it.

## Open
- (none logged)
