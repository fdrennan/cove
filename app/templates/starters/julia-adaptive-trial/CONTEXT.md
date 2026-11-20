# Adaptive Trial Designer · __HOST__
A Julia app, started from Cove's template: a Bayesian response-adaptive
multi-arm trial with a binary outcome, set against the fixed design that
randomises everyone equally and analyses once. Arm 1 is the control; each
arm's response rate has a Beta(1, 1) prior and a beta-binomial posterior.
After an equal-allocation burn-in, patients go to the arm the posterior
favours, by Thompson sampling or by P(best)^c with c = n / (2 n_max). Every
few patients there is an interim look: an experimental arm with P(best)
above the efficacy threshold wins and the trial stops; one below the
futility threshold is dropped, and when every experimental arm is dropped
the trial stops. One trial plays patient by patient: the posterior densities
fill the screen, with the allocation and the patients per arm below.
"Operating characteristics" runs thousands of trials of both designs on
every core, under the null (every arm at the control's rate) and under the
rates chosen, and sets type-I error, power, expected sample size and
patients on the best arm side by side. By default the efficacy threshold is
calibrated by simulation so the adaptive design's type-I error matches
alpha, which is what makes the comparison fair.

## Files
- server.jl - HTTP.jl: GET / (the page), GET /health, POST /api/trial
  ({rates, n, burnin, every, rule: "thompson"|"pbest", eff, fut, alpha, seed}
  -> each patient's arm, outcome and allocation probabilities, the looks
  with P(best) per arm, how the trial stopped, and the fixed design on the
  same rates), POST /api/oc ({same, sims, calibrate} -> {null, alt} x
  {adaptive, fixed}: reject rate, power, expected patients, patients on the
  best arm, responders, stopping reasons, per-arm selection, a histogram of
  trial sizes; and the threshold used). P(best) is integrated on a 500-point
  grid, so it has no Monte Carlo noise. The fixed design tests each arm
  against control with a one-sided two-proportion z-test at alpha / (K - 1).
  Distributions.jl for the beta and binomial draws and the normal quantile.
- static/index.html - the page: posterior densities on a full-screen canvas,
  labelled by arm with the true rate marked under the axis; allocation and
  patients-per-arm plots and the operating-characteristics table in glass
  panels; a glass card that folds (on a phone it starts folded); no external
  scripts
- run.sh - loads Julia from the ndexr stack (env/Julia/1.11.7, else
  Julia/1.11.7 and the stack's Julia site library); nothing is installed
- app.env - language, kind (port), port and health path, read by the verbs
- build.sh, up.sh, down.sh, env.sh - precompile the packages server.jl loads;
  start in the background and wait for /health (pid and log in .run/); stop;
  load Julia from the ndexr stack

## Run
`./run.sh` serves on 127.0.0.1:$APP_PORT, the port app.env gives this
project (`PORT=<n> ./run.sh` overrides it for one run); `./up.sh` and
`./down.sh` start and stop it in the background. Measured 2026-10-26 on
24 threads, control 30%, arms 30% and 45%, 300 patients, burn-in 60, looks
every 30, 10,000 trials per design and scenario, Thompson with the threshold
calibrated to alpha 0.05 (0.941): type-I error 5.1% adaptive, 4.9% fixed;
power 59% for both; 219 patients expected against 300; 59% of patients on
the better arm against 33%; 5.7 s for all 60,000 trials with the box at load
average 60.

## Built on
Thompson, W. R. (1933), "On the Likelihood that One Unknown Probability
Exceeds Another in View of the Evidence of Two Samples", Biometrika 25(3/4).
Thall, P. F. and Wathen, J. K. (2007), "Practical Bayesian adaptive
randomisation in clinical trials", European Journal of Cancer 43(5).
Berry, S. M., Carlin, B. P., Lee, J. J. and Müller, P. (2010), "Bayesian
Adaptive Methods for Clinical Trials", Chapman & Hall/CRC.

## Logging
Logging: log.jl (`Log.info`), one format across languages; LOG_LEVEL=DEBUG for more.

## Auth
You and the people you share it with (Project › Access sets who; a new site is never public). The server listens on 127.0.0.1 only; nginx in front decides who
reaches it.

## Open
- (none logged)
