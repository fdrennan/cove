# My Inlet · __HOST__
Your own Inlet - ndexr's fork of the open-source RStudio IDE - running as this
site and yours to reshape: its look, keys, snippets and defaults are files in
this project. Once it has a purpose, rename the first line and rewrite this
paragraph.

## Files
- config/rstudio-prefs.json - preferences; Inlet's Global Options write here too
- config/themes/*.rstheme - editor themes (my-inlet.rstheme is the default)
- config/keybindings/{rstudio,editor}_bindings.json - shortcuts
- config/snippets/*.snippets - snippets per language
- run.sh - rserver from `ml Inlet`, as you, on 127.0.0.1:APP_PORT, config/ as
  RSTUDIO_CONFIG_HOME; state in .run/inlet (not committed), rserver's sockets in
  ~/.local/state/ndexr-inlet/<id> (a socket path over 108 bytes stops it)
- recipe.sh - copies Inlet's easyconfig and every patch into recipe/, for
  changes config/ cannot make
- rebuild.sh - builds Inlet alone from recipe/ with EasyBuild (10 minutes
  on a quiet build box, about an hour on a busy shared one: it compiles RStudio); every dependency comes from the ndexr stack in CVMFS. It runs the
  stack's own EasyBuild (activate.sh in CVMFS: the suite's filter-deps and
  hooks) and installs into ~/.local/easybuild/<this host>, whose modules
  env.sh loads first, so Down then Up runs your build. Delete that folder to
  go back.
- app.env, build.sh, up.sh, down.sh, env.sh - how it is checked, started, stopped

## Seeing a change
A theme or snippet shows on reload of the page; a preference on the next
session (Session > Restart R, or Down then Up).

## Auth
Owner only (APP_ACCESS=owner, set when the site is published). Inlet itself
has no login: anyone who reaches it works as you, with your files and a
terminal. Share it under Project > Access only with people you would hand
your account.

## Licence
Inlet is RStudio (AGPL-3.0) with ndexr's patches; its source offer is linked
from Help in every session.
