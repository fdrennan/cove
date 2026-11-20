# My VS Code · __HOST__
Your own VS Code in the browser - code-server, from the ndexr stack - running
as this site and yours to reshape: its settings, keys and snippets are files
in this project, and code-server itself can be patched and rebuilt. Once it
has a purpose, rename the first line and rewrite this paragraph.

## Files
- config/settings.json - settings; VS Code's own Settings write here too
- config/keybindings.json - shortcuts
- config/snippets/*.json - snippets per language
- recipe.sh - copies code-server's easyconfig (and any patches) from the
  installed module into recipe/
- rebuild.sh - builds code-server alone from recipe/ with EasyBuild: every
  dependency comes from the ndexr stack in CVMFS. It runs the stack's own
  EasyBuild (activate.sh in CVMFS: the suite's filter-deps and hooks) and
  installs into ~/.local/easybuild/<this host>, whose modules env.sh loads
  first, so Down then Up runs your build. Delete that folder to go back.
- .cove/app.env, build.sh, up.sh, down.sh, env.sh, run.sh - how it is
  checked, started, stopped. State (extensions, history) is .cove/run/vscode.

## Extensions
The stack's code-server-extensions module (R, Python, Quarto) is installed
on first start. Extensions you add from the Extensions view stay in
.cove/run/vscode/extensions.

## Seeing a change
A setting applies as you save it; keybindings and snippets on reload of the
page; a rebuilt code-server after Down, then Up.

## Logging
run.sh writes the start line in the one format across languages; log.js (`const log = require('./log')`) is there for any Node code added, LOG_LEVEL=DEBUG for more.

## Auth
Owner only (APP_ACCESS=owner, set when the site is published). code-server
runs with --auth none: anyone who reaches it works as you, with your files
and a terminal. Share it under Project > Access only with people you would
hand your account.

## Licence
code-server is MIT (Coder), built on Code - OSS (MIT, Microsoft).
