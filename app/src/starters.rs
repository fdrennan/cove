//! What a new project can start as (operator, 2026-10-24: "Cove should feel
//! like a scientific application launcher ... select something interesting to
//! build, choose the language and framework, and immediately get a working
//! application in the IDE"). One table, read by the selector (ide.rs
//! `kind_chooser`) and by the server that lays the files down (host.rs
//! `create_domain`, through `Way::starter`).
//!
//! Each way is its own app (operator, 2026-10-26: "improve the uniqueness of
//! our projects"): no idea is ported to a second language. A way with no
//! `starter` is shown and cannot be picked.

/// The four worlds the project explorer groups categories into (operator,
/// 2026-10-26: "Organize projects into worlds rather than a flat list ...
/// thematic, not restrictive" - search still crosses them all).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum World {
    Discover,
    Intelligence,
    Create,
    Engineer,
}

/// Each world, in the explorer's order: its name and what it holds.
pub const WORLDS: &[(World, &str, &str)] = &[
    (World::Discover, "Discover", "Genomics, trials, epidemics and the physics of chaos"),
    (World::Intelligence, "Intelligence", "Machine learning, prediction, optimization and text"),
    (World::Create, "Create", "Dashboards, design systems and interactive tools"),
    (World::Engineer, "Engineer", "APIs, services and dependency graphs"),
];

/// One language and framework for a category, and the template that makes it.
pub struct Way {
    pub lang: &'static str,
    pub framework: &'static str,
    /// What you get, in a few words.
    pub app: &'static str,
    /// host.rs `create_domain`'s template id; None = not built yet.
    pub starter: Option<&'static str>,
}

pub struct Category {
    pub title: &'static str,
    /// Which of the four worlds the explorer groups it under.
    pub world: World,
    pub what: &'static str,
    pub ways: &'static [Way],
}

const fn ready(lang: &'static str, framework: &'static str, app: &'static str, starter: &'static str) -> Way {
    Way { lang, framework, app, starter: Some(starter) }
}


/// The template ids create_domain knows. The first three are compiled in by
/// hand (host.rs); every other id is a folder, templates/starters/<id>/,
/// embedded by build.rs (2026-10-25, operator: "expand on the languages
/// available for the projects people can start here ... each language should
/// have many different ones of different themes").
pub const SHINY_CHAOS: &str = "shiny-chaos";
pub const SHINY_BLANK: &str = "shiny-blank";
pub const RUST_APP: &str = "rust";
pub const FOLDER_STARTERS: &[&str] = &[
    "shiny-epidemic",
    "shiny-trial",
    "shiny-genomics",
    "shiny-dashboard",
    "shiny-finance",
    "shiny-signals",
    "shiny-text",
    "shiny-natures-lab",
    "shiny-geyser",
    "shiny-workbench",
    "shiny-feed",
    "node-colorlab",
    "node-orbital",
    "node-canvas",
    "node-dolphin",
    "python-pendulum",
    "python-digits",
    "python-detective",
    "python-outbreak",
    "python-topics",
    "python-cells",
    "python-apiwatch",
    "python-api",
    "python-dashboard",
    "python-ml",
    "julia-patterns",
    "julia-heat",
    "julia-optim",
    "julia-adaptive-trial",
    "julia-api",
    "julia-fractals",
    "julia-finance",
    "rust-api",
    "rust-particles",
    "rust-netwatch",
    "rust-jellyfish",
];

pub const CATEGORIES: &[Category] = &[
    Category {
        title: "Interactive Science",
        world: World::Discover,
        what: "Simulations, reactive charts and experiments you steer with a slider.",
        ways: &[
            ready("R", "Shiny", "Chaos Lab: the Lorenz system in base R, two runs parting live", SHINY_CHAOS),
            ready("Rust", "egui, browser and desktop", "Chaos Lab: the same equations in Rust, every frame", RUST_APP),
            ready("Python", "FastAPI and SciPy", "Double Pendulum Lab: a fan of pendulums a hair apart, their divergence measured, energy kept honest", "python-pendulum"),
            ready("React", "React and Node", "Orbital: the real Solar System from JPL's orbital elements - scrub through dates, find the next Mars opposition", "node-orbital"),
            ready("Rust", "tiny_http and rayon", "Particle Universe: thousands of bodies pulling on each other across every core - galaxies collide, energy kept honest", "rust-particles"),
            ready("Julia", "HTTP.jl and canvas", "Heat Lab: paint heaters, chillers and walls on a plate and watch heat spread, checked against the 2αt law", "julia-heat"),
        ],
    },
    Category {
        title: "Epidemics & Populations",
        world: World::Discover,
        what: "How an outbreak spreads, peaks and fades, and what vaccination changes.",
        ways: &[
            ready("R", "Shiny", "Epidemic Lab: an SIR model with R0, recovery and vaccination", "shiny-epidemic"),
            ready("Python", "FastAPI and SciPy", "Outbreak Simulator: 2,000 people on a map, households and hubs; close schools, distance, vaccinate, test mid-run", "python-outbreak"),
        ],
    },
    Category {
        title: "Clinical Trials & Statistics",
        world: World::Discover,
        what: "Simulated trials: power, sample size and what p-values really do.",
        ways: &[
            ready("R", "Shiny", "Trial Simulator: power against sample size, effect estimates, p-values", "shiny-trial"),
            ready("Julia", "HTTP.jl and Distributions.jl", "Adaptive Trial Designer: Bayesian response-adaptive arms against a fixed design, thousands of trials on every core", "julia-adaptive-trial"),
        ],
    },
    Category {
        title: "Statistical Exploration",
        world: World::Discover,
        what: "Data turned over by hand: densities, mixtures, models, and what they predict.",
        ways: &[
            ready("R", "Shiny", "Geyser Laboratory: Old Faithful's eruptions - a density whose bandwidth you drag, a mixture fitted by EM you step through, the next eruption predicted", "shiny-geyser"),
            ready("R", "Shiny", "Model Workbench: build regression formulas term by term on R's datasets - lm/glm fits, diagnostics, added-variable plots, AIC/anova/cross-validated comparison", "shiny-workbench"),
        ],
    },
    Category {
        title: "Genomics",
        world: World::Discover,
        what: "Gene expression between two groups: tests, corrections, volcano and MA plots.",
        ways: &[
            ready("R", "Shiny", "Expression Explorer: per-gene tests with BH, a clickable volcano plot", "shiny-genomics"),
            ready("R", "Shiny", "Nature's Laboratory: genetic variants as natural drug trials - knockouts, ancestry, a variant read as a trial, the drug pipeline", "shiny-natures-lab"),
            ready("Python", "FastAPI and scikit-learn", "Cell Atlas: single-cell RNA-seq from QC to clusters and marker genes, scored against the truth", "python-cells"),
        ],
    },
    Category {
        title: "Machine Learning",
        world: World::Intelligence,
        what: "Models learning a boundary between classes, and how well they generalise.",
        ways: &[
            ready("Python", "FastAPI and scikit-learn", "ML Playground: five models on moons, circles and blobs", "python-ml"),
            ready("Python", "FastAPI and scikit-learn", "Digit Lab: draw a digit, three models read it live with their confidence and the 8x8 they saw", "python-digits"),
        ],
    },
    Category {
        title: "Data Exploration",
        world: World::Intelligence,
        what: "A table you have never seen, read for you: its columns, correlations, outliers and surprises.",
        ways: &[
            ready("Python", "FastAPI and pandas", "Dataset Detective: four classic datasets explored - correlations, outliers, Simpson's paradox, found for you", "python-detective"),
        ],
    },
    Category {
        title: "Optimization",
        world: World::Intelligence,
        what: "Finding the lowest point of a landscape, and how differently each method gets there.",
        ways: &[
            ready("Julia", "HTTP.jl, Optim and ForwardDiff", "Optimizer Race: six methods race to the minimum from where you click, derivatives by automatic differentiation", "julia-optim"),
        ],
    },
    Category {
        title: "Fractals & Complex Systems",
        world: World::Discover,
        what: "Simple rules, intricate results: fractal sets and self-organising patterns.",
        ways: &[
            ready("Julia", "HTTP.jl and canvas", "Fractal Explorer: Mandelbrot and Julia sets on every core", "julia-fractals"),
            ready("Julia", "HTTP.jl and canvas", "Pattern Lab: Gray-Scott reaction-diffusion on every core - spots, coral, worms, mazes; paint new chemical", "julia-patterns"),
        ],
    },
    Category {
        title: "Finance & Monte Carlo",
        world: World::Intelligence,
        what: "Random price paths, option prices by simulation and by formula.",
        ways: &[
            ready("R", "Shiny", "Monte Carlo Pricer: paths, Black-Scholes, convergence", "shiny-finance"),
            ready("Julia", "HTTP.jl and canvas", "Monte Carlo Pricer: millions of paths on every core, against Black-Scholes", "julia-finance"),
        ],
    },
    Category {
        title: "Signals & Fourier",
        world: World::Discover,
        what: "Build a signal from waves and noise, see its spectrum, filter it.",
        ways: &[
            ready("R", "Shiny", "Fourier Lab: components, FFT spectrum, a low-pass filter", "shiny-signals"),
        ],
    },
    Category {
        title: "Text Analysis",
        world: World::Intelligence,
        what: "Words, frequencies and sentences in a passage, counted and drawn.",
        ways: &[
            ready("R", "Shiny", "Text Lab: word frequencies, sentence lengths, readability", "shiny-text"),
            ready("Python", "FastAPI and scikit-learn", "Topic Map: NMF and LDA topics over 91 abstracts, a map of documents, more-like-this and paste-your-own", "python-topics"),
        ],
    },
    Category {
        title: "Design & Color",
        world: World::Create,
        what: "Colour as science: perceptual spaces, palettes, contrast and how others see them.",
        ways: &[
            ready("React", "React and Node", "Color Laboratory: OKLCH palettes and gradients, WCAG and APCA contrast, colour-blindness simulation, a live themed mock-up", "node-colorlab"),
        ],
    },
    Category {
        title: "Productivity & Collaboration",
        world: World::Create,
        what: "Tools for the work around the science: boards, plans and live shared state.",
        ways: &[
            ready("React", "React and Node", "Project Canvas: a lab's task board - drag cards between stages, checklists, a burndown, every open tab in sync", "node-canvas"),
            ready("R", "Shiny", "Lab Network: a team feed laid out like the professional networks - notifications with unread counts, Jobs · My posts · Mentions, a profile card, search and posting", "shiny-feed"),
        ],
    },
    Category {
        title: "Games & Play",
        world: World::Create,
        what: "Arcade games for a phone or a keyboard, each with a score board the site's players share.",
        ways: &[
            ready("React", "React and Node", "Dolphin Jump: leap out of the waves for stars, dodge jellyfish, build combos", "node-dolphin"),
            ready("Rust", "WebAssembly, no bindgen", "Jellyfish Drift: pulse through coral gaps for pearls - the physics in Rust, compiled to WebAssembly", "rust-jellyfish"),
        ],
    },
    Category {
        title: "Dependency Explorer",
        world: World::Engineer,
        what: "Software, the packages it needs and where each came from, as a graph.",
        ways: &[
            ready("Rust", "egui, browser and desktop", "Dependency Universe: the ndexr stack's catalog as an atlas (the starter's second tab)", RUST_APP),
        ],
    },
    Category {
        title: "Dashboard",
        world: World::Create,
        what: "Metrics, live data, monitoring and research results.",
        ways: &[
            ready("R", "Shiny", "Machine Dashboard: load, memory and the R session, live", "shiny-dashboard"),
            ready("Python", "FastAPI and psutil", "System Dashboard: cores, memory, disk and network, live", "python-dashboard"),
            ready("Rust", "tiny_http", "Network Observatory: throughput, TCP states, listening ports and latency probes, read straight from /proc by Rust", "rust-netwatch"),
        ],
    },
    Category {
        title: "Web Service & API",
        world: World::Engineer,
        what: "Endpoints and calculation engines other programs call.",
        ways: &[
            ready("Python", "FastAPI", "Scientific API: statistics, fits, distributions and ODEs, with /docs", "python-api"),
            ready("Python", "FastAPI, pydantic and asyncio", "API Observatory: a service that watches itself - live p50/p95/p99, error rates, a load generator, contract checks", "python-apiwatch"),
            ready("Rust", "tiny_http", "Scientific API: statistics, fits, primes and Lorenz, fast", "rust-api"),
            ready("Julia", "HTTP.jl", "Scientific API: statistics, distributions, fits and ODEs", "julia-api"),
        ],
    },
    Category {
        title: "Blank",
        world: World::Create,
        what: "An empty page with the platform's shell around it, to start from nothing.",
        ways: &[ready("R", "Shiny", "One heading and the site's shell", SHINY_BLANK)],
    },
];

/// How many ways of a category can be created today.
pub fn ready_count(c: &Category) -> usize {
    c.ways.iter().filter(|w| w.starter.is_some()).count()
}

/// Points of the Lorenz attractor (x, z), for the Interactive Science card's
/// picture: the same equations the Chaos Lab starters solve.
pub fn lorenz_sketch(n: usize) -> Vec<(f32, f32)> {
    let (s, r, b, h) = (10.0f64, 28.0f64, 8.0f64 / 3.0, 0.01f64);
    let (mut x, mut y, mut z) = (1.0f64, 1.0f64, 1.0f64);
    let mut out = Vec::with_capacity(n);
    for i in 0..(n + 200) {
        let (dx, dy, dz) = (s * (y - x), x * (r - z) - y, x * y - b * z);
        x += dx * h;
        y += dy * h;
        z += dz * h;
        if i >= 200 {
            out.push((x as f32, z as f32));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_ready_way_names_a_template_create_domain_knows() {
        for c in CATEGORIES {
            for w in c.ways {
                if let Some(s) = w.starter {
                    assert!([SHINY_CHAOS, SHINY_BLANK, RUST_APP].contains(&s) || FOLDER_STARTERS.contains(&s), "{} / {} {}: {s}", c.title, w.lang, w.framework);
                }
            }
        }
        assert!(CATEGORIES.iter().filter(|c| ready_count(c) > 0).count() >= 3);
        // Every world holds something that can be made today.
        for (w, name, _) in WORLDS {
            assert!(CATEGORIES.iter().any(|c| c.world == *w && ready_count(c) > 0), "world {name} is empty");
        }
    }

    #[test]
    fn the_sketch_stays_on_the_attractor() {
        let p = lorenz_sketch(1500);
        assert_eq!(p.len(), 1500);
        assert!(p.iter().all(|(x, z)| x.abs() < 30.0 && *z > 0.0 && *z < 60.0), "left the attractor");
    }
}
