# Dataset Detective · __HOST__
A Python app, started from Cove's template: exploratory data analysis with
pandas on four tables that ship inside scikit-learn (iris, wine, breast cancer,
diabetes), so it opens on data with no upload and no network. Pick a table and
see every column's type, missing count, summary and shape; a correlation
heatmap (Pearson or Spearman); a histogram with density and box plot, split by
a category; a scatter of any two columns coloured by a third, with the pooled
and per-group fits; and findings the server surfaces by itself (strongest
correlations, what separates the groups, outliers by z-score and IQR, skew,
blanks, class imbalance). A slider blanks cells at random so the effect of
missing data on every number shows. The server answers JSON; the page draws
each chart on canvas. A starting point: describe what this site is for here,
and point it at your own table.

Why Python: pandas is the reference tool for this work. One `describe`,
`corr`, `skew` or `groupby` replaces a loop, missing values are skipped
pairwise without being asked, and scipy supplies the tests (Shapiro-Wilk,
Spearman, least squares, kernel density) in a line each.

## Files
- app.py - FastAPI and the statistics: DATASETS (add one and the page shows
  it), GET /api/menu, GET /api/profile (`dataset, missing, seed, z, iqr,
  method` -> columns, corr, findings), GET /api/column (`col, bins, by, log`
  -> histogram per group, density, box, outlier rows, Shapiro-Wilk p), GET
  /api/scatter (`x, y, color` -> points, least-squares fit, Pearson, Spearman,
  per-group fits)
- static/index.html, static/style.css, static/app.js - the page: glass panel
  with the tables, sliders and findings; cards for the column table, heatmap,
  distribution and scatter, drawn on canvas with hover read-outs
- run.sh - loads Python from the ndexr stack and serves with uvicorn

## Run
`./run.sh` serves on 127.0.0.1:$PORT (default 8800): `PORT=8810 ./run.sh`.
Python, fastapi, uvicorn, numpy, pandas, scipy and scikit-learn come from the
stack's env set (env/Python/3.13.15, else env/r4.6_py3.13-release); nothing is
pip-installed and nothing is downloaded: the four tables are files inside
scikit-learn.

Logging: log.py (from log import log as applog, since `log` names a log-scale option here), one format across languages; LOG_LEVEL=DEBUG for more.

## Built on
- Tukey, J. W. (1977). *Exploratory Data Analysis*. Addison-Wesley. The box
  plot and its 1.5 × IQR fences.
- McKinney, W. (2010). Data structures for statistical computing in Python.
  *Proc. 9th Python in Science Conf.*, 56-61. pandas.
- Fisher, R. A. (1936). The use of multiple measurements in taxonomic problems.
  *Annals of Eugenics* 7(2), 179-188. The iris table.
- Forina, M. et al. (1988). PARVUS: an extendible package for data
  exploration, classification and correlation. The wine table (UCI).
- Street, W. N., Wolberg, W. H. & Mangasarian, O. L. (1993). Nuclear feature
  extraction for breast tumor diagnosis. *Proc. SPIE* 1905, 861-870. The
  Wisconsin breast cancer table.
- Efron, B., Hastie, T., Johnstone, I. & Tibshirani, R. (2004). Least angle
  regression. *Annals of Statistics* 32(2), 407-499. The diabetes table.
- Shapiro, S. S. & Wilk, M. B. (1965). An analysis of variance test for
  normality. *Biometrika* 52(3-4), 591-611.
- Simpson, E. H. (1951). The interpretation of interaction in contingency
  tables. *JRSS B* 13(2), 238-241. The scatter flags a pooled trend that
  reverses inside every group.

## Auth
You and the people you share it with (Project › Access sets who; a new site is never public). The server listens on 127.0.0.1 only; nginx in front decides who
reaches it.

## Open
- (none logged)
