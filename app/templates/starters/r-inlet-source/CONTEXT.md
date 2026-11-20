# Inlet from source · __HOST__
RStudio's open-source code, and Inlet - ndexr's fork of it - as the commits
that make it, in a project where you work on the code directly and build it.
For reading how the IDE works, changing it, and seeing each change run. Once
it has a purpose, rename the first line and rewrite this paragraph.

## How it works
1. `./source.sh` (once): src/ becomes a git repository - `upstream` (the
   exact RStudio release the stack builds, checked by sha256), `inlet`
   (one commit per Inlet patch, in the recipe's order) and `work`, your
   branch, checked out. `git -C src log --oneline upstream..inlet` lists every
   Inlet change; `git -C src show <commit>` shows one.
2. Edit files in src/ - RStudio's own C++, Java (GWT), R and TypeScript.
   Commit on `work` as you go; each commit becomes one patch.
3. `./rebuild.sh`: `recipe.sh` writes recipe/ - Inlet's easyconfig with your
   commits (and anything not committed yet) added after Inlet's patches -
   then the stack's own EasyBuild builds Inlet from it, every dependency from
   CVMFS: 10 minutes on a quiet build box, about an hour on a busy shared one
   (measured 57 min). Down, then Up, serves your build here.

You never write a patch by hand: they are made from your commits, and
recipe/ is the record of your work saved with the project. src/ itself is not
saved (500 MB, its own repository); `./source.sh` and recipe/ rebuild it.

## Files
- source.sh, recipe.sh, rebuild.sh - the three steps above
- recipe/ - your work as an EasyBuild recipe (written by recipe.sh)
- config/ - Inlet's preferences, themes, keybindings, snippets (as My Inlet)
- .cove/ - how it is checked, started and stopped; run.sh serves the build
  you made, or the stack's Inlet before you have one

Logging: run.sh writes its start line in the one format across languages (r/log.r for R code here: log$log_info; LOG_LEVEL=DEBUG for more); rserver's own logs stay its own.

## Where things live
- Your build: ~/.local/easybuild/<this host> (delete it to go back)
- The RStudio tarball: ~/.local/easybuild/sources/i/Inlet (shared with
  EasyBuild, downloaded once)

## Auth
Owner only (APP_ACCESS=owner). Inlet itself has no login: anyone who reaches
it works as you. Share it under Project > Access only with people you would
hand your account.

## Licence
RStudio is AGPL-3.0, and so is anything built from it. This project is
yours; if you let others use a build of it over the network, AGPL section 13
asks you to offer them its source - recipe/ and the release it names.
