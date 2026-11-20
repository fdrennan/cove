# Lab Network · __HOST__
A Shiny site, started from Cove's template: a professional feed for a team,
laid out like the networks people already use. A bar across the top (the
site's mark, search, Home · Network · Jobs · Messaging · Notifications with its
unread count · Me), the person's card on the left, their notifications in the
middle, and a promoted card with the footer on the right. Rows are unread
until clicked, a row's "..." dismisses it, the pills (All · Jobs · My posts ·
Mentions) narrow the list and count what is unread in each, and a post you
write lands at the top of My posts. The search box narrows the feed as you
type. On a phone the side columns step away and the feed fills the screen.
Change first: `FEED` at the top of module.r - your own team in `people`, the
companies in `orgs`, and the kinds of news in `templates`.

## Files
- module.r - the whole site, a Shiny module (the ndexr module contract:
  `ui_module` / `server_module`, wrapped in `measured`). Four branches, each a
  `ui_`/`server_` pair:
  - `nav` - the top bar; returns what was searched, shows the feed's unread
    count on Notifications. The other tabs say they are yours to build.
  - `profile` - the person's card from `FEED$me`
  - `feed` - the notifications: `make_feed()` builds a visitor's starting
    rows (one per notification: kind, who, text, action, when, read) from a
    fixed seed; the server keeps them in a `reactiveVal`, so marking read,
    dismissing and posting are one row change each
  - `side` - the promoted card and the footer
  Save module.r and reload: the router picks the change up, no deploy.
- app.env, build.sh - a module app (no port of its own); build.sh checks that
  module.r parses

Logging: r/log.r (log$log_info, log_fn in every ui_/server_), one format across languages; LOG_LEVEL=DEBUG for more.

## Built on
Base R and Shiny only, from the stack's R; no data file, key or database.
Avatars are initials on a colour taken from the name; the icons are inline
SVG paths, so nothing is fetched from elsewhere.

## Auth
You and the people you share it with (Project › Access sets who; a new site is never public).

## Open
- (none logged)
