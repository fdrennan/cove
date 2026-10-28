# Optimizer Race · __HOST__
Six optimizers race from one start point to the minimum of a classic test
surface - Rosenbrock's banana valley, Himmelblau's four minima, Rastrigin's
egg box of local minima, Beale's flat valleys - drawn as a full-screen heat
map with contour lines. Click the map to choose a start; gradient descent,
momentum, Adam, Nelder-Mead, BFGS and Newton run side by side, each path in
its own colour, and the board ranks them by steps with the final value and
the minimum each one reached. The step size and step limit sliders rerun the
race live. At the start point the card shows f, its gradient, its Hessian
and the Hessian's eigenvalues (bowl, saddle or hilltop), with the error a
central finite difference makes beside ForwardDiff's.

What Julia is good at, shown here: automatic differentiation. Each surface is
written once, as plain Julia source text; the page prints that text, and
the server splices it into one generic method that runs on Float64 and on
ForwardDiff's dual numbers alike. Every gradient and Hessian, for the
hand-written gradient descent, momentum and Adam loops and for Optim.jl's
BFGS and Newton, comes from ForwardDiff. No derivative in this app is
derived by hand.

## Files
- server.jl - HTTP.jl: GET / (the page), GET /health, GET /api/surfaces
  (each surface's source text, domain, default start and known minima),
  GET /api/surface (?id, n -> log10(1 + f) on an n x n grid and marching
  squares contour segments), POST /api/race ({surface, x, y, iters, scale}
  -> per optimizer: path, steps, final point and value, gradient norm,
  status, nearest known minimum, f/g/h call counts, microseconds; plus the
  ForwardDiff gradient, Hessian and eigenvalues at the start). The six runs
  go one per thread. Optim.jl for Nelder-Mead, BFGS and Newton; ForwardDiff.jl
  for every derivative.
- static/index.html - the page: the heat map and racing paths on a
  full-screen canvas (the race clock runs on log steps, so Newton's handful
  and gradient descent's thousands play out together), a glass card that
  folds (on a phone it starts folded); click a row to hide that racer
- run.sh - serves server.jl in the foreground with one thread per core
- app.env - language, kind (port), port and health path, read by the verbs
- build.sh, up.sh, down.sh, env.sh - precompile the packages server.jl loads;
  start in the background and wait for /health (pid and log in .run/); stop;
  load Julia from the ndexr stack (env/Julia/1.11.7, else Julia/1.11.7 and
  the stack's Julia site library); nothing is installed

## Run
`./run.sh` serves on 127.0.0.1:$APP_PORT, the port app.env gives this
project (`PORT=<n> ./run.sh` for one run). The first start
compiles Optim and ForwardDiff for the race and takes one to three minutes;
after that a race answers in 5-50 ms. Measured 2026-10-26 on 24 threads:
Rosenbrock from (-1.2, 1): Newton 24 steps, BFGS 41, Nelder-Mead 89 and
Adam 3,930 reach (1, 1) with f below 1e-12; gradient descent and momentum
are still crawling along the valley at 5,000. Himmelblau from (-4, -4),
(4, -3), (1, 1) and (-4, 4) lands on (-3.779310, -3.283186),
(3.584428, -1.848126), (3, 2) and (-2.805118, 3.131312), f below 1e-14.

## Built on
- Rosenbrock, H. H. (1960), "An automatic method for finding the greatest or
  least value of a function", The Computer Journal 3(3).
- Himmelblau, D. M. (1972), Applied Nonlinear Programming, McGraw-Hill.
- Rastrigin, L. A. (1974), Systems of Extremal Control, Nauka.
- Beale, E. M. L. (1958), "On an iterative method for finding a local
  minimum of a function of more than one variable", Princeton tech. report 25.
- Polyak, B. T. (1964), "Some methods of speeding up the convergence of
  iteration methods", USSR Comp. Math. and Math. Physics 4(5).
- Kingma, D. P. and Ba, J. (2015), "Adam: A Method for Stochastic
  Optimization", ICLR.
- Nelder, J. A. and Mead, R. (1965), "A simplex method for function
  minimization", The Computer Journal 7(4).
- Nocedal, J. and Wright, S. J. (2006), Numerical Optimization, 2nd ed.,
  Springer (BFGS, Newton with line search).
- Revels, J., Lubin, M. and Papamarkou, T. (2016), "Forward-Mode Automatic
  Differentiation in Julia", arXiv:1607.07892 (ForwardDiff.jl).
- Mogensen, P. K. and Riseth, A. N. (2018), "Optim: A mathematical
  optimization package for Julia", JOSS 3(24).

## Auth
Login-gated. The server listens on 127.0.0.1 only; nginx in front decides who
reaches it.

## Open
- (none logged)
