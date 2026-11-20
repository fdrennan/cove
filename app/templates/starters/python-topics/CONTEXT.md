# Topic Map · __HOST__
A Python app, started from Cove's template: the themes in a corpus of
documents, found by scikit-learn. corpus.json holds 91 short abstracts from six
fields (astronomy, genetics, epidemiology, climate, materials, neuroscience).
The server builds TF-IDF over n-grams with stop words removed and min/max
document-frequency cuts, fits k topics by NMF or LDA, and lays the documents
out in two dimensions (TruncatedSVD, then t-SNE) marked by dominant topic.
Each topic lists its top words and its coherence (UMass and NPMI, computed
from document co-occurrence in numpy); a sweep over every k shows which
number of topics holds together best, and "vs fields" scores how well the
topics recover the fields the models never saw. Pick a document for the ones
most like it (cosine on TF-IDF); paste any text to see where it lands and
which topic claims it. Change first: corpus.json - put your own documents in
its place (title, field, text; the field is any label you want the topics
scored against) and every view follows.

## Files
- app.py - FastAPI and the work, in three cached stages (features, topics,
  layout): GET /api/menu (corpus, colours, defaults), POST /api/model
  (`{"method","k","ngram","min_df","max_df","perplexity","seed"}` -> docs with
  x, y, topic and mix; topics with words, umass, npmi, size, field; ami),
  POST /api/sweep (same body -> coherence and ami for k = 2..8),
  POST /api/similar (+ `"doc","n"` -> neighbours), POST /api/place
  (+ `"text","n"` -> x, y, topic, mix, neighbours, terms)
- corpus.json - the corpus: title, field, text per document
- static/index.html, static/style.css, static/app.js - the page: the map
  fills the window, topics and "more like this" sit in cards beneath it, the
  controls and the paste box in a glass panel
- run.sh - loads Python from the ndexr stack and serves with uvicorn
- app.env - language, kind (port), port and health path, read by the verbs
- build.sh, up.sh, down.sh, env.sh - byte-compile and import-check app.py;
  start in the background and wait for /health (pid and log in .run/); stop;
  load Python from the ndexr stack

## Run
`./run.sh` serves on 127.0.0.1:$APP_PORT, the port app.env gives this
project (`PORT=<n> ./run.sh` overrides it for one run); `./up.sh` and
`./down.sh` start and stop it in the background.
Python, fastapi, uvicorn, numpy and scikit-learn come
from the stack's env set (env/Python/3.13.15, else env/r4.6_py3.13-release);
nothing is pip-installed.

Logging: log.py (from log import log), one format across languages; LOG_LEVEL=DEBUG for more.

## Built on
- The corpus was written for this starter, from the textbook record of each
  field's results (Hubble-era cosmology, Hodgkin-Huxley, Snow's cholera map,
  Doll and Hill's physicians cohort and the like); it quotes no publication
  and is dedicated to the public domain (CC0).
- NMF: Lee, D. D. & Seung, H. S. (1999), "Learning the parts of objects by
  non-negative matrix factorization", Nature 401, 788-791.
- LDA: Blei, D. M., Ng, A. Y. & Jordan, M. I. (2003), "Latent Dirichlet
  allocation", Journal of Machine Learning Research 3, 993-1022.
- t-SNE: van der Maaten, L. & Hinton, G. (2008), "Visualizing data using
  t-SNE", Journal of Machine Learning Research 9, 2579-2605.
- UMass coherence: Mimno, D. et al. (2011), "Optimizing semantic coherence in
  topic models", Proceedings of EMNLP 2011, 262-272.
- NPMI: Bouma, G. (2009), "Normalized (pointwise) mutual information in
  collocation extraction", Proceedings of GSCL, 31-40.
- scikit-learn: Pedregosa, F. et al. (2011), JMLR 12, 2825-2830.

## Auth
You and the people you share it with (Project › Access sets who; a new site is never public). The server listens on 127.0.0.1 only; nginx in front decides who
reaches it.

## Open
- (none logged)
