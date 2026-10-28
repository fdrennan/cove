#!/usr/bin/env bash
# Build: precompile every package server.jl loads, so `up` starts in seconds
# instead of minutes. Safe to run again; Julia skips what is compiled.
#
#   ./build.sh
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

# Every top-level `using`/`import` line, not only the first: reading one line
# skipped Distributions, GLM and OrdinaryDiffEq in julia-api, and its first up
# compiled them inside the health wait (127 s, 2026-10-26). `using Foo: a, b`
# loads Foo alone; `using .Local` is a module of this file, not a package.
pkgs="$(sed -nE 's/^(using|import)[[:space:]]+//p' server.jl |
  sed -E 's/:.*//' | tr ',' '\n' | sed -E 's/^[[:space:]]+|[[:space:]]+$//g' |
  grep -E '^[A-Za-z_][A-Za-z0-9_]*$' | awk '!seen[$0]++' | paste -sd, - | sed 's/,/, /g')"
echo "build: precompiling $pkgs"
start=$SECONDS
julia --startup-file=no -e "using $pkgs"
echo "build: done in $((SECONDS - start)) s"
