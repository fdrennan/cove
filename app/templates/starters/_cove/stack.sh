# Sourced by .cove/<language>/env.sh: which software stack this project's
# modules come from, and the three functions env.sh uses to load them.
# (operator, 2026-11-26, on r/HPC: Cove should "consume either the ndexr
# stack, the EESSI stack or the Compute Canada stack using ml".) Cove's own
# table of them is app/src/host/stacks.rs; keep the two in step.
#
# One word picks the stack: COVE_STACK, else $HOME/.config/ndexr/cove-stack
# (Cove's Settings write it), else the first of these that is mounted:
#   ndexr          /opt/ndexr/stack/use.sh
#   eessi          /cvmfs/software.eessi.io/versions/2023.06/init/bash
#   computecanada  /cvmfs/soft.computecanada.ca/config/profile/bash.sh
#                  then: module load StdEnv/2023

cove_stack_init() {
  case "$1" in
    ndexr) echo /opt/ndexr/stack/use.sh ;;
    eessi) echo /cvmfs/software.eessi.io/versions/2023.06/init/bash ;;
    computecanada) echo /cvmfs/soft.computecanada.ca/config/profile/bash.sh ;;
  esac
}
cove_stack_pick() {
  local want="${COVE_STACK:-}" s
  # In $HOME, not $XDG_CONFIG_HOME: Cove gives each session its own of that.
  [ -n "$want" ] || want="$(tr -d '[:space:]' 2>/dev/null < "$HOME/.config/ndexr/cove-stack")"
  case "$want" in ndexr|eessi|computecanada) echo "$want"; return ;; esac
  for s in ndexr eessi computecanada; do
    [ -r "$(cove_stack_init "$s")" ] && { echo "$s"; return; }
  done
  echo ndexr
}
COVE_STACK_NAME="$(cove_stack_pick)"
case "$COVE_STACK_NAME" in
  ndexr) COVE_STACK_LABEL="the ndexr stack" ;;
  eessi) COVE_STACK_LABEL="EESSI" ;;
  computecanada) COVE_STACK_LABEL="the Compute Canada stack" ;;
esac
export COVE_STACK_NAME

# Is the chosen stack here? Says which and where when it is not.
stack_here() {
  [ -r "$(cove_stack_init "$COVE_STACK_NAME")" ] || { echo "env: $COVE_STACK_LABEL is not mounted ($(cove_stack_init "$COVE_STACK_NAME")) - mount it, or pick another stack in Cove (Tools > Global Options > Stack)" >&2; exit 127; }
}

# Put `ml` and the stack's modules in this shell.
stack_enter() {
  . "$(cove_stack_init "$COVE_STACK_NAME")" >/dev/null 2>&1
  if [ "$COVE_STACK_NAME" = computecanada ]; then
    module load StdEnv/2023 >/dev/null 2>&1
  fi
  return 0
}

# The programs a module name of the ndexr stack's stands for: what to ask
# another stack for, at its own default version. EESSI spells them as
# EasyBuild does; Compute Canada in lower case (stack_ml tries both).
cove_programs() {
  case "$1" in
    env/r*_py*) echo "R Python" ;;
    env/*/*) local p="${1#env/}"; echo "${p%%/*}" ;;
    *) echo "${1%%/*}" ;;
  esac
}

cove_said() {
  printf '%s INFO    [env] %s from %s\n' "$(date -u '+%Y-%m-%d %H:%M:%S')" "$(ml -t list 2>&1 | grep -i "^${1%%/*}/" | tail -1)" "$COVE_STACK_LABEL"
}

# The app runs in its stack environment or not at all - never on whatever the
# host happens to have (operator, 2026-11-19: "the application must absolutely
# run with its proper ml environment"). The names given are the ndexr stack's;
# the first that loads wins. On another stack, the first whose programs all
# load at that stack's default version wins.
stack_ml() {
  local m p n ok
  for m in "$@"; do
    if ml "$m" >/dev/null 2>&1; then
      cove_said "$m"
      return 0
    fi
  done
  if [ "$COVE_STACK_NAME" != ndexr ]; then
    for m in "$@"; do
      ok=1
      for p in $(cove_programs "$m"); do
        if ml "$p" >/dev/null 2>&1; then
          cove_said "$p"
        elif n="$(printf '%s' "$p" | tr '[:upper:]' '[:lower:]')" && ml "$n" >/dev/null 2>&1; then
          cove_said "$n"
        else
          ok=0
          break
        fi
      done
      [ "$ok" = 1 ] && return 0
    done
  fi
  echo "env: none of $* loads from $COVE_STACK_LABEL - try: ml spider $(cove_programs "$1" | cut -d' ' -f1)" >&2
  exit 127
}
