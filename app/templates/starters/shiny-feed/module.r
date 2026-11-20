# __HOST__/module.r
#
# Lab Network: a professional feed for a team, laid out like the networks
# people already know - a bar across the top, the person's card on the left,
# their notifications in the middle, and what else is on offer on the right.
#
#   nav       the top bar: the site's mark, search, Home · Network · Jobs ·
#             Messaging · Notifications (with its unread count) · Me
#   profile   the person's card: banner, initials, name, headline, place
#   feed      All · Jobs · My posts · Mentions over the notifications; a row
#             is unread until clicked, its "..." dismisses it, a post you
#             write lands at the top of My posts
#   side      a promoted card and the footer
#
# Everything is generated here in R, per visitor, from a fixed seed: no data
# file, key or database. Change first: FEED$people and FEED$templates - your
# own team and the kinds of news it shares.

FEED <- list(
  me = list(name = "Alex Rivera", headline = "Research Software Engineer · R, HPC, Shiny",
            place = "Boulder, Colorado", org = "Harbor Lab"),
  people = c("Maya Okafor", "Jonah Weiss", "Priya Raman", "Tomás Herrera", "Lena Fischer",
             "Kenji Sato", "Amara Nwosu", "Diego Morales", "Sofia Lindqvist", "Omar Haddad"),
  orgs = c("Northwind Labs", "Helix Analytics", "Cobalt Systems", "Riverbend Bio", "Tessera Health", "Lumen Genomics"),
  # kind, how it reads ({who} a person, {org} a company, {what} a topic), and
  # the button it carries ("" for none).
  templates = data.frame(
    kind = c("jobs", "jobs", "jobs", "network", "network", "posts", "posts", "mentions", "mentions"),
    text = c("<b>{what} programmer</b>: new opportunities in <b>North America</b>",
             "<b>{what}</b>: new opportunities in <b>United States</b>",
             "See <b>10 jobs</b> similar to one you viewed: <b>Senior Software Engineer</b> at <b>{org}</b>",
             "<b>{who}</b> followed you. View their profile.",
             "<b>{who}</b> and <b>2 other people</b> viewed your profile.",
             "<b>{who}</b> and <b>4 others</b> reacted to your post.",
             "<b>{who}</b> commented on your post: “this is exactly what our {what} team needs”",
             "<b>{who}</b> mentioned you in a comment about <b>{what}</b>.",
             "<b>{who}</b> mentioned you in a post: “ask Alex about {what} on HPC”"),
    action = c("View jobs", "View jobs", "View jobs", "View profile", "See all views", "", "Reply", "Reply", "View post"),
    stringsAsFactors = FALSE
  ),
  topics = c("r", "rust", "r (programming language)", "Shiny", "Quarto", "GxP validation", "Bioconductor"),
  # How many unread rows a visitor starts with.
  unread = 3L,
  loaded = Sys.time()
)

# Initials, and a colour of their own from the name, for an avatar.
initials <- function(name) {
  parts <- strsplit(name, "\\s+")[[1]]
  if (length(parts) == 1) return(toupper(substr(parts, 1, 2)))
  toupper(paste(substr(parts[c(1, length(parts))], 1, 1), collapse = ""))
}
hue_of <- function(name) {
  h <- sum(utf8ToInt(name)) %% 360
  grDevices::hcl(h = h, c = 45, l = 55)
}

# "12m", "2h", "3d": how long ago, as the feed shows it.
ago <- function(at, now = Sys.time()) {
  s <- as.numeric(difftime(now, at, units = "secs"))
  ifelse(s < 3600, paste0(pmax(1, floor(s / 60)), "m"),
         ifelse(s < 86400, paste0(floor(s / 3600), "h"), paste0(floor(s / 86400), "d")))
}

# A visitor's starting feed: one row per notification, newest first.
make_feed <- function(n = 18L, seed = 7L, now = Sys.time()) {
  set.seed(seed)
  tp <- FEED$templates[sample.int(nrow(FEED$templates), n, replace = TRUE), ]
  who <- sample(FEED$people, n, replace = TRUE)
  org <- sample(FEED$orgs, n, replace = TRUE)
  what <- sample(FEED$topics, n, replace = TRUE)
  fill <- function(t, w, o, x) gsub("{what}", x, gsub("{org}", o, gsub("{who}", w, t, fixed = TRUE), fixed = TRUE), fixed = TRUE)
  mins <- sort(cumsum(stats::rexp(n, 1 / 40)) + 5)
  data.frame(
    id = sprintf("n%03d", seq_len(n)),
    kind = tp$kind,
    who = ifelse(tp$kind == "jobs", org, who),
    html = mapply(fill, tp$text, who, org, what, USE.NAMES = FALSE),
    action = tp$action,
    at = now - mins * 60,
    read = seq_len(n) > FEED$unread,
    stringsAsFactors = FALSE
  )
}

# ── drawing helpers ──────────────────────────────────────────────────────────

# Small line icons for the top bar, drawn inline so nothing is fetched.
ICON <- c(
  home = "M3 11l9-7 9 7v9a1 1 0 0 1-1 1h-5v-6H9v6H4a1 1 0 0 1-1-1z",
  network = "M8 11a3 3 0 1 0 0-6 3 3 0 0 0 0 6zm8 0a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM2 20c0-3.3 2.7-6 6-6s6 2.7 6 6zm12 0c0-2.2-.9-4.1-2.3-5.5A6 6 0 0 1 22 20z",
  jobs = "M9 4h6a1 1 0 0 1 1 1v2h4a1 1 0 0 1 1 1v11a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V8a1 1 0 0 1 1-1h4V5a1 1 0 0 1 1-1zm1 3h4V6h-4z",
  messaging = "M4 4h16a1 1 0 0 1 1 1v11a1 1 0 0 1-1 1H9l-5 4v-4H4a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1z",
  bell = "M12 22a2.5 2.5 0 0 0 2.4-2h-4.8a2.5 2.5 0 0 0 2.4 2zm7-6V11a7 7 0 0 0-5-6.7V3a2 2 0 0 0-4 0v1.3A7 7 0 0 0 5 11v5l-2 2v1h18v-1z",
  search = "M10 3a7 7 0 0 1 5.6 11.2l5.1 5.1-1.4 1.4-5.1-5.1A7 7 0 1 1 10 3zm0 2a5 5 0 1 0 0 10 5 5 0 0 0 0-10z"
)

icon <- function(name, size = 22) {
  box::use(shiny = shiny[HTML])
  shiny$HTML(sprintf('<svg viewBox="0 0 24 24" width="%d" height="%d" aria-hidden="true"><path fill="currentColor" d="%s"/></svg>', size, size, ICON[[name]]))
}

avatar <- function(name, size = 48, square = FALSE) {
  box::use(shiny = shiny[tags])
  shiny$tags$span(class = paste("av", if (square) "sq"), style = sprintf("width:%dpx;height:%dpx;background:%s;font-size:%dpx", size, size, hue_of(name), round(size * 0.38)), initials(name))
}

# ── nav ──────────────────────────────────────────────────────────────────────

ui_nav <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(shiny = shiny[NS, tags, textInput, uiOutput])
  ns <- shiny$NS(id)
  item <- function(key, label, badge = NULL) {
    shiny$tags$a(class = "nav-item", href = "#", `data-key` = key, onclick = sprintf("Shiny.setInputValue('%s', '%s', {priority: 'event'}); return false;", ns("go"), key),
                 shiny$tags$span(class = "nav-ico", icon(if (key == "notifications") "bell" else key), badge), shiny$tags$span(label))
  }
  shiny$tags$header(class = "lf-top",
    shiny$tags$div(class = "lf-top-in",
      shiny$tags$span(class = "lf-mark", toupper(substr("__HOST__", 1, 2))),
      shiny$tags$label(class = "lf-search", icon("search", 18),
        shiny$tags$input(id = ns("q"), type = "search", class = "shiny-input-text", placeholder = "I'm looking for…")),
      shiny$tags$nav(class = "lf-nav",
        item("home", "Home"), item("network", "Network"), item("jobs", "Jobs"), item("messaging", "Messaging"),
        item("notifications", "Notifications", shiny$uiOutput(ns("badge"), inline = TRUE)),
        shiny$tags$a(class = "nav-item", href = "#", avatar(FEED$me$name, 26), shiny$tags$span("Me ▾"))
      )
    )
  )
}

# The bar reports what was searched and the unread count it is handed.
server_nav <- function(id, unread) {
  box::use(./r/log)
  log$log_fn()
  box::use(shiny = shiny[moduleServer, renderUI, reactive, tags, observeEvent, showNotification])
  shiny$moduleServer(id, function(input, output, session) {
    output$badge <- shiny$renderUI({
      n <- unread()
      if (n > 0) shiny$tags$b(class = "badge", n)
    })
    shiny$observeEvent(input$go, {
      page <- paste0(toupper(substr(input$go, 1, 1)), substring(input$go, 2))
      if (input$go != "notifications") shiny$showNotification(sprintf("%s is yours to build: add a ui_/server_ pair for it in module.r.", page), duration = 4)
    })
    shiny$reactive(trimws(if (is.null(input$q)) "" else input$q))
  })
}

# ── profile ──────────────────────────────────────────────────────────────────

ui_profile <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(shiny = shiny[NS, tags])
  me <- FEED$me
  shiny$tags$aside(class = "lf-left",
    shiny$tags$section(class = "card profile",
      shiny$tags$div(class = "banner", shiny$tags$span(class = "tier", "Premium")),
      shiny$tags$div(class = "pav", avatar(me$name, 84)),
      shiny$tags$h2(me$name),
      shiny$tags$p(class = "headline", me$headline),
      shiny$tags$p(class = "place", me$place),
      shiny$tags$p(class = "org", shiny$tags$span(class = "orgmark"), me$org)
    ),
    shiny$tags$section(class = "card link-card", shiny$tags$span("Manage notifications"), shiny$tags$span("›"))
  )
}

server_profile <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(shiny = shiny[moduleServer])
  shiny$moduleServer(id, function(input, output, session) NULL)
}

# ── feed ─────────────────────────────────────────────────────────────────────

ui_feed <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(shiny = shiny[NS, tags, uiOutput, textAreaInput, actionButton])
  ns <- shiny$NS(id)
  shiny$tags$main(class = "lf-mid",
    shiny$tags$section(class = "card pills", shiny$uiOutput(ns("pills"), inline = TRUE)),
    shiny$tags$section(class = "card compose",
      avatar(FEED$me$name, 44),
      shiny$tags$div(class = "compose-box",
        shiny$tags$textarea(id = ns("post"), class = "form-control", rows = 2, placeholder = "Start a post: what is your team working on?"),
        shiny$actionButton(ns("send"), "Post", class = "btn-post"))
    ),
    shiny$tags$section(class = "card list", shiny$uiOutput(ns("rows")))
  )
}

server_feed <- function(id, query) {
  box::use(./r/log)
  log$log_fn()
  box::use(
    shiny = shiny[moduleServer, reactiveVal, reactive, renderUI, observeEvent, tags, HTML, invalidateLater, updateTextAreaInput],
    htmltools[htmlEscape]
  )
  shiny$moduleServer(id, function(input, output, session) {
    ns <- session$ns
    feed <- shiny$reactiveVal(make_feed())
    tab <- shiny$reactiveVal("all")
    shiny$observeEvent(input$tab, tab(input$tab))

    shown <- shiny$reactive({
      f <- feed()
      t <- tab()
      if (t != "all") f <- f[f$kind == t, , drop = FALSE]
      q <- tolower(query())
      if (nzchar(q)) f <- f[grepl(q, tolower(paste(f$who, gsub("<[^>]+>", "", f$html))), fixed = TRUE), , drop = FALSE]
      f
    })

    output$pills <- shiny$renderUI({
      f <- feed()
      tabs <- c(all = "All", jobs = "Jobs", posts = "My posts", mentions = "Mentions")
      shiny$tags$div(class = "pill-row", lapply(names(tabs), function(k) {
        n <- if (k == "all") sum(!f$read) else sum(!f$read & f$kind == k)
        shiny$tags$button(class = paste("pill", if (tab() == k) "on"),
          onclick = sprintf("Shiny.setInputValue('%s', '%s', {priority: 'event'})", ns("tab"), k),
          tabs[[k]], if (n > 0) shiny$tags$b(class = "pill-n", n))
      }), if (any(!f$read)) shiny$tags$button(class = "pill quiet",
          onclick = sprintf("Shiny.setInputValue('%s', 1, {priority: 'event'})", ns("all_read")), "Mark all read"))
    })

    output$rows <- shiny$renderUI({
      shiny$invalidateLater(60000)
      f <- shown()
      if (!nrow(f)) return(shiny$tags$p(class = "empty", "Nothing here yet."))
      now <- Sys.time()
      shiny$tags$ul(class = "rows", lapply(seq_len(nrow(f)), function(i) {
        r <- f[i, ]
        set_ <- function(what) sprintf("event.stopPropagation(); Shiny.setInputValue('%s', '%s', {priority: 'event'})", ns(what), r$id)
        shiny$tags$li(class = paste("nrow", if (!r$read) "unread"), onclick = set_("open"),
          if (!r$read) shiny$tags$span(class = "dot"),
          avatar(r$who, 48, square = r$kind == "jobs"),
          shiny$tags$div(class = "body",
            shiny$tags$p(shiny$HTML(r$html)),
            if (nzchar(r$action)) shiny$tags$button(class = "act", onclick = set_("act"), r$action)),
          shiny$tags$div(class = "meta",
            shiny$tags$span(class = "when", ago(r$at, now)),
            shiny$tags$button(class = "more", title = "Dismiss", onclick = set_("drop"), "⋯"))
        )
      }))
    })

    mark <- function(ids, read = TRUE) {
      f <- feed()
      f$read[f$id %in% ids] <- read
      feed(f)
    }
    shiny$observeEvent(input$open, mark(input$open))
    shiny$observeEvent(input$all_read, mark(feed()$id))
    shiny$observeEvent(input$act, mark(input$act))
    shiny$observeEvent(input$drop, {
      f <- feed()
      feed(f[f$id != input$drop, , drop = FALSE])
    })
    # A post of your own: at the top of My posts, read, from you.
    shiny$observeEvent(input$send, {
      text <- trimws(if (is.null(input$post)) "" else input$post)
      if (!nzchar(text)) return()
      f <- feed()
      row <- data.frame(id = sprintf("p%d", nrow(f) + as.integer(Sys.time())), kind = "posts", who = FEED$me$name,
                        html = paste0("<b>You</b> posted: ", htmlEscape(text)), action = "", at = Sys.time(),
                        read = TRUE, stringsAsFactors = FALSE)
      feed(rbind(row, f))
      tab("posts")
      shiny$updateTextAreaInput(session, "post", value = "")
    })

    shiny$reactive(sum(!feed()$read))
  })
}

# ── side ─────────────────────────────────────────────────────────────────────

ui_side <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(shiny = shiny[NS, tags])
  shiny$tags$aside(class = "lf-right",
    shiny$tags$section(class = "card promo",
      shiny$tags$div(class = "promo-top", shiny$tags$span("Promoted ⋯")),
      shiny$tags$div(class = "promo-faces", avatar(FEED$me$name, 64), avatar(FEED$orgs[3], 64, square = TRUE)),
      shiny$tags$p(shiny$tags$b(sprintf("%s, explore relevant opportunities with %s", strsplit(FEED$me$name, " ")[[1]][1], FEED$orgs[3]))),
      shiny$tags$p(class = "muted", "Get the latest jobs and industry news"),
      shiny$tags$button(class = "follow", "Follow")
    ),
    shiny$tags$footer(class = "lf-foot",
      shiny$tags$p("About · Accessibility · Help Center · Privacy & Terms · Advertising"),
      shiny$tags$p(sprintf("__HOST__ © %s", format(Sys.Date(), "%Y"))))
  )
}

server_side <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(shiny = shiny[moduleServer])
  shiny$moduleServer(id, function(input, output, session) NULL)
}

FEED_CSS <- "
.lf { background: #f4f2ee; color: #1d2226; min-height: 100vh; font-family: -apple-system, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif; }
.lf-top { background: #fff; border-bottom: 1px solid #e0dfdc; position: sticky; top: 0; z-index: 10; }
.lf-top-in { max-width: 1128px; margin: 0 auto; display: flex; align-items: center; gap: 12px; padding: 0 16px; height: 56px; }
.lf-mark { background: #0a66c2; color: #fff; font-weight: 800; border-radius: 6px; width: 38px; height: 38px; display: grid; place-items: center; font-size: 17px; flex: none; }
.lf-search { display: flex; align-items: center; gap: 8px; background: #edf3f8; border-radius: 18px; padding: 0 14px; height: 36px; width: 280px; color: #5e6670; margin: 0; }
.lf-search input { border: 0; background: transparent; outline: none; flex: 1; font-size: 15px; color: #1d2226; }
.lf-nav { display: flex; margin-left: auto; height: 100%; }
.nav-item { display: flex; flex-direction: column; align-items: center; justify-content: center; min-width: 76px; color: #5e6670 !important; text-decoration: none !important; font-size: 12.5px; border-bottom: 2px solid transparent; }
.nav-item:hover { color: #1d2226 !important; }
.nav-item[data-key=notifications] { color: #1d2226 !important; border-bottom-color: #1d2226; }
.nav-ico { position: relative; height: 24px; }
.badge { position: absolute; top: -4px; right: -10px; background: #cb112d; color: #fff; border-radius: 10px; font-size: 11px; padding: 1px 6px; }
.lf-body { max-width: 1128px; margin: 24px auto; display: grid; grid-template-columns: 225px minmax(0, 1fr) 300px; gap: 24px; padding: 0 16px; align-items: start; }
.card { background: #fff; border: 1px solid #e0dfdc; border-radius: 10px; overflow: hidden; }
.lf-left .card + .card, .lf-mid .card + .card, .lf-right .card + .lf-foot { margin-top: 10px; }
.profile { text-align: left; padding-bottom: 14px; }
.banner { height: 58px; background: linear-gradient(120deg, #a0b4b7, #5b6b8c 55%, #3a2f4f); position: relative; }
.tier { position: absolute; right: 10px; top: 6px; color: #fff; font-weight: 700; font-size: 13px; }
.pav { margin: -42px 0 0 16px; position: relative; z-index: 1; }
.pav .av { border: 3px solid #fff; }
.profile h2 { font-size: 20px; margin: 8px 16px 2px; font-weight: 700; }
.headline, .place, .org { margin: 0 16px; font-size: 13.5px; color: #3d4349; }
.place { color: #5e6670; margin-top: 4px; }
.org { margin-top: 10px; font-weight: 600; display: flex; align-items: center; gap: 8px; }
.orgmark { width: 16px; height: 16px; background: #a100ff; clip-path: polygon(0 0, 100% 50%, 0 100%, 25% 50%); }
.link-card { padding: 14px 16px; display: flex; justify-content: space-between; font-weight: 600; font-size: 15px; }
.av { display: inline-grid; place-items: center; border-radius: 50%; color: #fff; font-weight: 700; flex: none; }
.av.sq { border-radius: 8px; }
.pills { padding: 12px 16px; }
.pill-row { display: flex; gap: 8px; flex-wrap: wrap; }
.pill { border: 1px solid #8c8c8c; background: #fff; color: #1d2226; border-radius: 18px; padding: 5px 14px; font-weight: 600; font-size: 15px; display: inline-flex; gap: 6px; align-items: center; }
.pill.on { background: #01754f; border-color: #01754f; color: #fff; }
.pill.quiet { border-color: transparent; color: #0a66c2; margin-left: auto; }
.pill-n { background: #cb112d; color: #fff; border-radius: 9px; font-size: 11px; padding: 0 6px; }
.compose { display: flex; gap: 12px; padding: 12px 16px; align-items: flex-start; }
.compose-box { flex: 1; display: flex; gap: 8px; align-items: flex-end; }
.compose-box textarea { border-radius: 18px; resize: vertical; font-size: 15px; }
.btn-post { background: #0a66c2 !important; color: #fff !important; border: 0; border-radius: 18px; font-weight: 600; padding: 6px 18px; }
.rows { list-style: none; margin: 0; padding: 0; }
.nrow { display: flex; gap: 12px; padding: 14px 16px 14px 26px; border-bottom: 1px solid #e8e6e3; position: relative; cursor: pointer; align-items: flex-start; }
.nrow:hover { background: #f8f8f6; }
.nrow.unread { background: #dce6f1; }
.nrow.unread:hover { background: #d0dded; }
.dot { position: absolute; left: 10px; top: 32px; width: 8px; height: 8px; border-radius: 50%; background: #0a66c2; }
.body { flex: 1; min-width: 0; }
.body p { margin: 0 0 8px; font-size: 15px; line-height: 1.4; }
.act { border: 1px solid #0a66c2; color: #0a66c2; background: transparent; border-radius: 18px; padding: 4px 14px; font-weight: 600; font-size: 15px; }
.act:hover { background: #e8f1fb; }
.meta { display: flex; flex-direction: column; align-items: flex-end; gap: 6px; color: #5e6670; font-size: 13px; }
.more { border: 0; background: transparent; font-size: 20px; line-height: 1; color: #5e6670; border-radius: 50%; width: 32px; height: 32px; }
.more:hover { background: #e9e9e9; }
.empty { padding: 24px; color: #5e6670; }
.promo { padding: 12px 16px 16px; text-align: center; }
.promo-top { text-align: right; font-size: 12px; color: #5e6670; }
.promo-faces { display: flex; justify-content: center; gap: 12px; margin: 8px 0; }
.promo p { font-size: 15px; margin: 6px 0; }
.muted { color: #5e6670; }
.follow { width: 100%; border: 1px solid #0a66c2; color: #0a66c2; background: #fff; border-radius: 20px; padding: 6px; font-weight: 600; font-size: 16px; margin-top: 8px; }
.lf-foot { text-align: center; font-size: 12px; color: #5e6670; padding: 8px; }
@media (max-width: 1000px) { .lf-body { grid-template-columns: minmax(0, 1fr) 300px; } .lf-left { display: none; } }
@media (max-width: 720px) {
  .lf-body { grid-template-columns: minmax(0, 1fr); margin-top: 10px; padding: 0 8px; } .lf-right { display: none; }
  .lf-top-in { gap: 8px; padding: 0 8px; }
  .lf-search { width: auto; flex: 1; min-width: 0; } .lf-search input { min-width: 0; width: 100%; }
  .nav-item > span:last-child { display: none; } .nav-item { min-width: 40px; }
  .lf-nav .nav-item:nth-child(1), .lf-nav .nav-item:nth-child(2), .lf-nav .nav-item:nth-child(4) { display: none; }
}
"

#' @export
ui_module <- function(id, request) {
  box::use(./r/log)
  log$log_fn()
  box::use(
    shiny = shiny[NS, tagList, tags, HTML],
    r / shell / shell
  )
  ns <- shiny$NS(id)
  shell$ui_shell(
    ns        = ns,
    request   = request,
    head_args = list(
      title      = "__HOST__",
      domain     = "__HOST__",
      stylesheet = "css/home.css"
    ),
    theme = "light",
    with_footer = FALSE,
    body = shiny$tagList(
      # HTML(): as plain text the child selectors (">") would be escaped.
      shiny$tags$style(shiny$HTML(FEED_CSS)),
      shiny$tags$div(class = "lf",
        ui_nav(ns("nav")),
        shiny$tags$div(class = "lf-body",
          ui_profile(ns("profile")),
          ui_feed(ns("feed")),
          ui_side(ns("side"))
        )
      )
    )
  )
}

.server_inner <- function(id = "__HOST__", parent_session) {
  box::use(r / shell / shell, ./r/log, shiny = shiny[reactiveVal, observe])
  shell$server_shell(id, parent_session, inner = function(input, output, session) {
    log$log_session(session)
    log$log_info("__HOST__: module server starting")
    # The bar's search narrows the feed; the feed's unread count is the bar's badge.
    unread <- shiny$reactiveVal(0L)
    query <- server_nav("nav", function() unread())
    server_profile("profile")
    n <- server_feed("feed", query)
    shiny$observe(unread(n()))
    server_side("side")
  })
}

#' @export
server_module <- local({
  box::use(r / observability / timing[measured])
  wrapped <- measured("__HOST__", .server_inner)
  function(id = "__HOST__", ...) wrapped(id, ...)
})
