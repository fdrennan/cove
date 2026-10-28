# Text Lab · __HOST__
A Shiny site, started from Cove's template: paste any text and see its words
counted, its sentences measured and its readability estimated. It opens on
the first paragraph of Darwin's On the Origin of Species (1859), and has
four more public-domain passages (Darwin's last paragraph, the Gettysburg
Address, Pride and Prejudice, Moby-Dick) to pick from. Change first:
`PASSAGES` - add your own texts and they join the picker and the reference
the keyness is measured against.

## Files
- module.r - the whole site, a Shiny module (the ndexr module contract:
  `ui_module` / `server_module`, wrapped in `measured`). Two branches, each a
  `ui_`/`server_` pair:
  - `words` - the most frequent words, or the most distinctive against the
    other passages (Dunning's log-likelihood keyness), full screen; the card holds the text and
    the counts, with Flesch reading ease
  - `patterns` - sentence lengths, and Zipf's law (frequency against rank,
    log-log, with the fitted slope)
  Each visual fills the window under the site's header, its controls in a
  glass card that folds to its title (a `<details>`; on a phone it starts
  folded). Save module.r and reload: the router picks the change up, no deploy.
- app.env, build.sh - a module app (no port of its own); build.sh checks that
  module.r parses

## Built on
Base R and Shiny only, from the stack's R; no dataset, key or database.
Flesch, R. (1948), "A new readability yardstick", J. Applied Psychology
32(3) (syllables are estimated, so the score is too). Zipf, G. K. (1949),
"Human Behavior and the Principle of Least Effort".

## Auth
Login-gated.

## Open
- (none logged)
