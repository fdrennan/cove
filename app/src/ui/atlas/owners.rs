//! Who looks after a package, read from the homepage its catalog record
//! carries (operator, 2026-10-21: "packages are too complex and need clear
//! views of ownership somehow, being able to visualize where other
//! competitors are building and owning in the visual space is also
//! important"). The account in its GitHub/GitLab address, the project in a
//! *.github.io or *.readthedocs.io site, else the site's own domain - then,
//! where that account is one of a known steward's, the steward's name (Posit
//! for r-lib, rstudio and the tidyverse; Meta for pytorch ...). A guess from a
//! URL, said as one: the card names the homepage it came from.

/// The account or site a homepage belongs to: "r-lib", "pytorch",
/// "numpy.org". Empty when there is no homepage, or it is only a registry's
/// page (CRAN, crates.io, docs.rs, PyPI), which says nothing about the owner.
pub(crate) fn account(home: &str) -> String {
    let h = home.trim().to_ascii_lowercase();
    let rest = h.split_once("://").map_or(h.as_str(), |(_, r)| r);
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.split(':').next().unwrap_or(host).trim_start_matches("www.");
    let first = path.split('/').find(|p| !p.is_empty()).unwrap_or("");
    if host.is_empty() {
        return String::new();
    }
    if matches!(host, "github.com" | "gitlab.com" | "bitbucket.org" | "codeberg.org" | "sr.ht" | "git.sr.ht") {
        return first.trim_start_matches('~').to_owned();
    }
    if matches!(host, "cran.r-project.org" | "cran.rstudio.com" | "crates.io" | "docs.rs" | "lib.rs" | "pypi.org" | "pypi.python.org") {
        return String::new();
    }
    for site in [".github.io", ".readthedocs.io", ".r-universe.dev", ".gitlab.io", ".codeberg.page"] {
        if let Some(sub) = host.strip_suffix(site) {
            return sub.rsplit('.').next().unwrap_or(sub).to_owned();
        }
    }
    if host.contains("bioconductor") {
        return "bioconductor".to_owned();
    }
    let parts: Vec<&str> = host.split('.').collect();
    // ggplot2.tidyverse.org, dplyr.r-lib.org: the site, not the package.
    if parts.len() >= 2 {
        let tail = parts[parts.len() - 2..].join(".");
        // A country's academic domain (ox.ac.uk): one level more.
        if matches!(parts[parts.len() - 2], "ac" | "co" | "edu" | "gov") && parts.len() >= 3 {
            return parts[parts.len() - 3..].join(".");
        }
        return tail;
    }
    host.to_owned()
}

/// The organisation behind an account, where it is a known one: the
/// companies and communities a pharmaceutical company weighs as suppliers
/// and competitors. Accounts not listed keep their own name.
pub(crate) fn steward(account: &str) -> Option<&'static str> {
    Some(match account {
        "r-lib" | "r-lib.org" | "rstudio" | "rstudio.com" | "tidyverse" | "tidyverse.org" | "posit-dev" | "posit.co" | "tidymodels" | "tidymodels.org" | "quarto-dev" | "quarto.org" | "shiny.posit.co" => "Posit",
        "pharmaverse" | "pharmaverse.org" => "pharmaverse",
        "insightsengineering" | "roche" | "genentech" => "Roche",
        "gsk-biostatistics" | "gsk" => "GSK",
        "novonordisk-opensource" | "novonordisk" => "Novo Nordisk",
        "atorus-research" => "Atorus",
        "openpharma" => "openpharma",
        "r-project.org" | "r-devel" | "wch" => "R Core",
        "bioconductor" => "Bioconductor",
        "ropensci" | "ropensci.org" => "rOpenSci",
        "mc-stan.org" | "stan-dev" => "Stan",
        "futureverse.org" | "futureverse" | "henrikbengtsson" => "futureverse",
        "r-dbi" | "r-dbi.org" => "R-DBI",
        "appsilon" => "Appsilon",
        "microsoft" | "revolutionanalytics" | "azure" | "microsoft.com" => "Microsoft",
        "google" | "googleapis" | "googlecloudplatform" | "tensorflow" | "jax-ml" | "google.com" => "Google",
        "pytorch" | "facebookresearch" | "facebook" | "meta-llama" | "pytorch.org" => "Meta",
        "nvidia" | "nvidia.com" => "NVIDIA",
        "aws" | "boto" | "awslabs" | "amazon.com" => "Amazon",
        "databricks" | "mlflow" | "databricks.com" => "Databricks",
        "anaconda" | "conda" | "anaconda.com" | "numba" => "Anaconda",
        "huggingface" | "huggingface.co" => "Hugging Face",
        "jupyter" | "jupyter.org" | "jupyterlab" | "ipython" => "Project Jupyter",
        "numpy" | "numpy.org" | "scipy" | "scipy.org" | "pandas-dev" | "pydata" | "pydata.org" | "matplotlib" | "matplotlib.org" | "scikit-learn" | "scikit-learn.org" => "NumFOCUS",
        "pypa" | "pypa.io" | "python" | "python.org" => "Python (PSF)",
        "pallets" | "palletsprojects.com" => "Pallets",
        "astral-sh" => "Astral",
        "pola-rs" | "pola.rs" => "Polars",
        "apache" | "apache.org" => "Apache",
        "rust-lang" | "rust-lang-nursery" | "rust-random" | "rust-num" | "rust-cli" | "rust-lang.org" => "Rust project",
        "tokio-rs" | "tokio.rs" => "Tokio",
        "servo" => "Servo",
        "mozilla" => "Mozilla",
        // Julia's communities, each its own (2026-10-22, operator: "julia
        // doesnt really have any groups or players in it"), by the GitHub
        // organisation its registry entry names.
        "julialang" | "julialang.org" => "JuliaLang",
        "juliabinarywrappers" => "JuliaBinaryWrappers",
        "sciml" => "SciML",
        "juliamath" => "JuliaMath",
        "juliadata" => "JuliaData",
        "juliaio" => "JuliaIO",
        "juliastats" => "JuliaStats",
        "juliaarrays" => "JuliaArrays",
        "juliagraphics" => "JuliaGraphics",
        "turinglang" => "Turing",
        "juliacollections" => "JuliaCollections",
        "juliadiff" => "JuliaDiff",
        "juliaplots" => "JuliaPlots",
        "makieorg" => "Makie",
        "juliageometry" => "JuliaGeometry",
        "juliapluto" => "Pluto",
        "juliaweb" => "JuliaWeb",
        "juliapackaging" => "JuliaPackaging",
        "juliadocs" => "JuliaDocs",
        "juliainterop" => "JuliaInterop",
        "juliapy" => "JuliaPy",
        "jump-dev" => "JuMP",
        "julialinearalgebra" => "JuliaLinearAlgebra",
        "juliaimages" => "JuliaImages",
        "fluxml" => "FluxML",
        _ => return None,
    })
}

/// The steward an owner name is, when it is one (the names `steward` gives).
pub(crate) fn steward_name(owner: &str) -> Option<&'static str> {
    const KNOWN: &[&str] = &[
        "Posit", "pharmaverse", "Roche", "GSK", "Novo Nordisk", "Atorus", "openpharma", "R Core", "Bioconductor", "rOpenSci", "Stan", "futureverse", "R-DBI", "Appsilon",
        "Microsoft", "Google", "Meta", "NVIDIA", "Amazon", "Databricks", "Anaconda", "Hugging Face", "Project Jupyter", "NumFOCUS", "Python (PSF)", "Pallets", "Astral", "Polars",
        "Apache", "Rust project", "Tokio", "Servo", "Mozilla", "JuliaLang", "JuliaBinaryWrappers", "SciML", "JuliaMath", "JuliaData", "JuliaIO", "JuliaStats",
        "JuliaArrays", "JuliaGraphics", "Turing", "JuliaCollections", "JuliaDiff", "JuliaPlots", "Makie", "JuliaGeometry", "Pluto", "JuliaWeb", "JuliaPackaging", "JuliaDocs",
        "JuliaInterop", "JuliaPy", "JuMP", "JuliaLinearAlgebra", "JuliaImages", "FluxML",
    ];
    KNOWN.iter().copied().find(|k| *k == owner)
}

/// The owner a package is drawn under: a known steward, else its account;
/// "unlisted" with no homepage that names one.
pub(crate) fn owner(home: &str) -> String {
    let a = account(home);
    match steward(&a) {
        Some(s) => s.to_owned(),
        None if a.is_empty() => UNLISTED.to_owned(),
        None => a,
    }
}

pub(crate) const UNLISTED: &str = "unlisted";
/// Accounts with too few packages to name: many small hands.
pub(crate) const INDEPENDENT: &str = "independent";

/// A language's packages by the role they play in the work (operator,
/// 2026-10-21: "Position should communicate role in the work. Complexity can
/// be additional information ... 'Commonly used' should eventually come from
/// observed use ... Until that information exists, the grouping should be
/// presented as a curated starting point"). On top the workflows and domain
/// stacks people choose to do a piece of science; in the middle the packages
/// they use directly; at the bottom the machinery under both.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum Tier {
    Supporting = 0,
    Everyday = 1,
    Workflow = 2,
}

impl Tier {
    /// The middle first: it is the language's own ring.
    pub(crate) const ALL: [Tier; 3] = [Tier::Everyday, Tier::Workflow, Tier::Supporting];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Tier::Workflow => "workflows and domain stacks",
            Tier::Everyday => "everyday packages",
            Tier::Supporting => "supporting packages and interfaces",
        }
    }

    pub(crate) fn what(self) -> &'static str {
        match self {
            Tier::Workflow => "the larger combinations people choose to do a piece of science: a modelling environment, a reporting system, an application framework, a domain toolkit",
            Tier::Everyday => "what people use directly to import, transform, analyse, model and visualise data",
            Tier::Supporting => "the machinery underneath: language interfaces, bindings, parsers, serialization, package infrastructure",
        }
    }
}

/// Workflows and domain stacks, named: the curated half of the top ring.
const WORKFLOW: &[&str] = &[
    // R
    "tidyverse", "tidymodels", "shiny", "rmarkdown", "quarto", "bookdown", "blogdown", "flexdashboard", "officer", "officedown", "targets", "drake", "golem", "rhino", "plumber",
    "teal", "admiral", "admiralonco", "admiralophtha", "admiralvaccine", "tern", "chevron", "rtables", "Tplyr", "gtsummary", "brms", "rstanarm", "cmdstanr", "rstan", "mlr3",
    "caret", "keras", "keras3", "tensorflow", "torch", "sparklyr", "h2o", "Seurat", "DESeq2", "edgeR", "limma", "scran", "scater", "SingleCellExperiment", "sf", "terra",
    "leaflet", "rsconnect", "pharmaRTF", "xportr", "metacore", "metatools", "datasetjson",
    // Python
    "jupyter", "jupyterlab", "notebook", "jupyter-server", "streamlit", "dash", "panel", "gradio", "snakemake", "scanpy", "scikit-learn", "transformers", "lightning",
    "pytorch-lightning", "tensorflow", "keras", "dask", "pyspark", "mlflow", "prefect", "apache-airflow", "statsmodels", "pymc", "xarray", "geopandas", "nbconvert", "sphinx",
    "shiny", "great-tables", "plotnine", "lifelines", "scikit-survival",
];

/// Supporting packages and interfaces, named: the curated half of the bottom.
const SUPPORTING: &[&str] = &[
    // R
    "Rcpp", "RcppArmadillo", "RcppEigen", "RcppParallel", "RcppTOML", "cpp11", "BH", "rlang", "vctrs", "glue", "cli", "R6", "withr", "magrittr", "lifecycle", "pillar", "fansi",
    "utf8", "crayon", "ellipsis", "pkgconfig", "jsonlite", "xml2", "XML", "curl", "RCurl", "openssl", "httr", "httr2", "digest", "base64enc", "mime", "sys", "processx", "ps",
    "callr", "fs", "rappdirs", "evaluate", "highr", "yaml", "stringi", "backports", "generics", "cachem", "memoise", "fastmap", "later", "promises", "httpuv", "websocket",
    "htmltools", "htmlwidgets", "jquerylib", "sass", "bslib", "fontawesome", "rprojroot", "desc", "pkgbuild", "pkgload", "remotes", "devtools", "testthat", "roxygen2",
    "commonmark", "markdown", "tinytex", "xfun", "codetools", "DBI", "odbc", "RSQLite", "RPostgres", "reticulate", "rstudioapi", "sourcetools", "uuid", "brew", "credentials",
    "gh", "gert", "askpass", "farver", "labeling", "munsell", "colorspace", "isoband", "systemfonts", "textshaping", "ragg", "gtable", "scales", "tzdb", "cpp11armadillo",
    "rstantools", "StanHeaders", "posterior", "otel", "mirai", "nanonext", "future", "globals", "listenv", "parallelly", "progressr", "bit", "bit64", "blob", "clipr",
    // Python
    "typing-extensions", "typing_extensions", "setuptools", "packaging", "six", "attrs", "cffi", "pycparser", "certifi", "urllib3", "idna", "charset-normalizer", "requests",
    "pip", "wheel", "pyparsing", "python-dateutil", "pytz", "tzdata", "filelock", "platformdirs", "importlib-metadata", "zipp", "markupsafe", "jinja2", "pyyaml", "toml",
    "tomli", "decorator", "wrapt", "cython", "pybind11", "nanobind", "meson", "meson-python", "ninja", "scikit-build-core", "hatchling", "flit-core", "poetry-core",
    "setuptools-scm", "pyarrow", "protobuf", "grpcio", "msgpack", "orjson", "cloudpickle", "fsspec", "rpy2", "traitlets", "tornado", "pyzmq", "jupyter-core", "jupyter-client",
    "ipykernel", "comm", "debugpy", "psutil", "click", "colorama", "rich", "tqdm", "pydantic", "pydantic-core", "annotated-types", "anyio", "sniffio", "h11", "httpcore",
    "httpx", "aiohttp", "multidict", "yarl", "frozenlist", "aiosignal", "async-timeout",
];

/// Everyday packages a classifier or a rule would put elsewhere: what people
/// use directly whatever its metadata says (numpy calls itself "Software
/// Development").
const EVERYDAY: &[&str] = &[
    "numpy", "scipy", "pandas", "matplotlib", "seaborn", "polars", "ipython", "sympy", "networkx", "h5py", "openpyxl", "sqlalchemy", "duckdb", "pillow", "opencv-python",
    "ggplot2", "dplyr", "tidyr", "readr", "data.table", "survival", "lme4", "Matrix", "MASS", "stringr", "lubridate", "purrr", "tibble", "haven", "readxl",
    "tokio", "serde", "serde_json", "clap", "reqwest", "axum", "ndarray", "arrow", "datafusion", "egui", "eframe", "rayon", "regex", "anyhow", "thiserror",
];

/// A package's role: named in a curated list; else, for a Python package,
/// its own top-level classifier; else an assembly of a large share of its
/// language (standing on half the most any of them stands on, with nothing
/// of its own language built on it) reads as a workflow; else, for Rust - a
/// language of building blocks - supporting, unless it is a program;
/// otherwise everyday.
pub(crate) fn role(lang: &str, name: &str, purpose: &str, used_by: usize, reach: usize, most: usize) -> Tier {
    if WORKFLOW.contains(&name) {
        return Tier::Workflow;
    }
    if SUPPORTING.contains(&name) {
        return Tier::Supporting;
    }
    if EVERYDAY.contains(&name) {
        return Tier::Everyday;
    }
    let top = purpose.split(" › ").next().unwrap_or("").trim();
    if lang == "Python" && matches!(top, "Software Development" | "Internet" | "System" | "Utilities" | "Text Processing" | "Security" | "Communications" | "Terminals" | "Typing" | "File Formats" | "Printing" | "Desktop Environment") {
        return Tier::Supporting;
    }
    if used_by == 0 && reach >= 8 && reach * 2 >= most {
        return Tier::Workflow;
    }
    match lang {
        "Rust" if purpose.contains("program") => Tier::Workflow,
        "Rust" => Tier::Supporting,
        _ => Tier::Everyday,
    }
}

/// Each package's role among `members` (one language's, indices into
/// `deps`): `role`, with how many of them stand on it and how many of them
/// it stands on (transitively) as the additional signal.
pub(crate) fn tiers(members: &[usize], lang: &str, name: impl Fn(usize) -> (String, String), deps: impl Fn(usize) -> Vec<usize>) -> Vec<Tier> {
    use std::collections::{HashMap, HashSet};
    let at: HashMap<usize, usize> = members.iter().enumerate().map(|(j, &k)| (k, j)).collect();
    let direct: Vec<Vec<usize>> = members.iter().map(|&k| deps(k).into_iter().filter_map(|d| at.get(&d).copied()).collect()).collect();
    let mut used = vec![0usize; members.len()];
    for ds in &direct {
        for &d in ds {
            used[d] += 1;
        }
    }
    let reach: Vec<usize> = (0..members.len())
        .map(|j| {
            let mut seen = HashSet::new();
            let mut stack = direct[j].clone();
            while let Some(k) = stack.pop() {
                if seen.insert(k) {
                    stack.extend(direct[k].iter().copied());
                }
            }
            seen.len()
        })
        .collect();
    let most = reach.iter().copied().max().unwrap_or(0).max(1);
    members
        .iter()
        .enumerate()
        .map(|(j, &k)| {
            let (n, p) = name(k);
            role(lang, &n, &p, used[j], reach[j], most)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn owners_from_homepages() {
        use super::owner;
        assert_eq!(owner("https://ggplot2.tidyverse.org"), "Posit");
        assert_eq!(owner("https://github.com/r-lib/rlang"), "Posit");
        assert_eq!(owner("https://pharmaverse.github.io/admiral/"), "pharmaverse");
        assert_eq!(owner("https://insightsengineering.github.io/tern/"), "Roche");
        assert_eq!(owner("https://pytorch.org"), "Meta");
        assert_eq!(owner("https://numpy.org"), "NumFOCUS");
        assert_eq!(owner("https://serde.rs"), "serde.rs");
        assert_eq!(owner("https://github.com/dtolnay/anyhow"), "dtolnay");
        assert_eq!(owner("https://sphinx.readthedocs.io/en/latest"), "sphinx");
        assert_eq!(owner("https://docs.rs/foo"), "unlisted");
        assert_eq!(owner(""), "unlisted");
        assert_eq!(owner("https://www.stats.ox.ac.uk/pub/MASS4/"), "ox.ac.uk");
    }

    #[test]
    fn three_roles() {
        use super::{role, Tier};
        assert_eq!(role("R", "tidyverse", "", 0, 30, 40), Tier::Workflow);
        assert_eq!(role("R", "Rcpp", "", 200, 0, 40), Tier::Supporting);
        // Used by many and still something people use directly.
        assert_eq!(role("R", "dplyr", "A Grammar of Data Manipulation", 40, 12, 40), Tier::Everyday);
        assert_eq!(role("Python", "numpy", "Software Development", 50, 0, 20), Tier::Everyday);
        assert_eq!(role("Python", "packaging", "Python package", 50, 0, 20), Tier::Supporting);
        assert_eq!(role("Python", "aiohappyeyeballs", "Internet", 1, 0, 20), Tier::Supporting);
        assert_eq!(role("Python", "pandas", "Scientific/Engineering", 20, 5, 20), Tier::Everyday);
        assert_eq!(role("Python", "scanpy", "Scientific/Engineering", 0, 15, 20), Tier::Workflow);
        // An assembly nothing else uses, standing on most of its language.
        assert_eq!(role("R", "somestack", "", 0, 25, 40), Tier::Workflow);
        assert_eq!(role("Rust", "itoa", "Rust crate", 90, 1, 30), Tier::Supporting);
        assert_eq!(role("Rust", "serde", "Rust crate", 90, 1, 30), Tier::Everyday);
        assert_eq!(role("Rust", "ripgrep", "Rust program (cargo install)", 0, 2, 30), Tier::Workflow);
    }
}
