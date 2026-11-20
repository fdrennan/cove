# __HOST__/module.r
#
# Text Lab: paste any text and see its words counted, its sentences measured
# and its readability estimated - in plain R, no packages.
#
#   words      the most frequent words, edge to edge; the card holds the text
#              and the counts: words, distinct words, sentences, how long they
#              run, and a readability score.
#   patterns   how long the sentences are, and Zipf's law: a word's frequency
#              falls about as one over its rank, a straight line of slope
#              near -1 on log-log axes.
#
# Readability is Flesch's reading ease, 206.835 - 1.015 (words per sentence)
# - 84.6 (syllables per word): 60-70 is plain English, below 30 is hard.
# Syllables are estimated (vowel groups, less a silent final e), so the score
# is an estimate too. Flesch, R. (1948), "A new readability yardstick",
# J. Applied Psychology 32(3). Zipf, G. K. (1949), "Human Behavior and the
# Principle of Least Effort".

TXT <- list(col = c(bar = "#5AC8FA", fit = "#F0DC78", hist = "#3DDC97", ink = "#e6e9ef"))

# Darwin, "On the Origin of Species" (1859), the opening of its introduction
# - in the public domain. Paste anything in its place.
SAMPLE <- paste(
  "When on board H.M.S. 'Beagle,' as naturalist, I was much struck with certain facts in the distribution",
  "of the inhabitants of South America, and in the geological relations of the present to the past",
  "inhabitants of that continent. These facts seemed to me to throw some light on the origin of species -",
  "that mystery of mysteries, as it has been called by one of our greatest philosophers. On my return home,",
  "it occurred to me, in 1837, that something might perhaps be made out on this question by patiently",
  "accumulating and reflecting on all sorts of facts which could possibly have any bearing on it. After five",
  "years' work I allowed myself to speculate on the subject, and drew up some short notes; these I enlarged",
  "in 1844 into a sketch of the conclusions, which then seemed to me probable: from that period to the",
  "present day I have steadily pursued the same object. I hope that I may be excused for entering on these",
  "personal details, as I give them to show that I have not been hasty in coming to a decision."
)

# More public-domain passages to pick from; each is also the others' yardstick
# for keyness (below).
PASSAGES <- list(
  "Darwin, the Origin's introduction (1859)" = SAMPLE,
  "Darwin, the Origin's last paragraph (1859)" = paste(
    "It is interesting to contemplate an entangled bank, clothed with many plants of many kinds, with birds",
    "singing on the bushes, with various insects flitting about, and with worms crawling through the damp earth,",
    "and to reflect that these elaborately constructed forms, so different from each other, and dependent on",
    "each other in so complex a manner, have all been produced by laws acting around us. These laws, taken in",
    "the highest sense, being Growth with Reproduction; Inheritance which is almost implied by reproduction;",
    "Variability from the indirect and direct action of the external conditions of life, and from use and",
    "disuse; a Ratio of Increase so high as to lead to a Struggle for Life, and as a consequence to Natural",
    "Selection, entailing Divergence of Character and the Extinction of less-improved forms. Thus, from the war",
    "of nature, from famine and death, the most exalted object which we are capable of conceiving, namely, the",
    "production of the higher animals, directly follows. There is grandeur in this view of life, with its",
    "several powers, having been originally breathed into a few forms or into one; and that, whilst this planet",
    "has gone cycling on according to the fixed law of gravity, from so simple a beginning endless forms most",
    "beautiful and most wonderful have been, and are being, evolved."
  ),
  "Lincoln, the Gettysburg Address (1863)" = paste(
    "Four score and seven years ago our fathers brought forth on this continent, a new nation, conceived in",
    "Liberty, and dedicated to the proposition that all men are created equal. Now we are engaged in a great",
    "civil war, testing whether that nation, or any nation so conceived and so dedicated, can long endure. We",
    "are met on a great battle-field of that war. We have come to dedicate a portion of that field, as a final",
    "resting place for those who here gave their lives that that nation might live. It is altogether fitting",
    "and proper that we should do this. But, in a larger sense, we can not dedicate - we can not consecrate -",
    "we can not hallow - this ground. The brave men, living and dead, who struggled here, have consecrated it,",
    "far above our poor power to add or detract. The world will little note, nor long remember what we say",
    "here, but it can never forget what they did here. It is for us the living, rather, to be dedicated here",
    "to the unfinished work which they who fought here have thus far so nobly advanced. It is rather for us to",
    "be here dedicated to the great task remaining before us - that from these honored dead we take increased",
    "devotion to that cause for which they gave the last full measure of devotion - that we here highly",
    "resolve that these dead shall not have died in vain - that this nation, under God, shall have a new birth",
    "of freedom - and that government of the people, by the people, for the people, shall not perish from the earth."
  ),
  "Austen, Pride and Prejudice, its opening (1813)" = paste(
    "It is a truth universally acknowledged, that a single man in possession of a good fortune, must be in",
    "want of a wife. However little known the feelings or views of such a man may be on his first entering a",
    "neighbourhood, this truth is so well fixed in the minds of the surrounding families, that he is considered",
    "the rightful property of some one or other of their daughters. \"My dear Mr. Bennet,\" said his lady to",
    "him one day, \"have you heard that Netherfield Park is let at last?\" Mr. Bennet replied that he had not.",
    "\"But it is,\" returned she; \"for Mrs. Long has just been here, and she told me all about it.\" Mr. Bennet",
    "made no answer. \"Do you not want to know who has taken it?\" cried his wife impatiently. \"You want to",
    "tell me, and I have no objection to hearing it.\" This was invitation enough."
  ),
  "Melville, Moby-Dick, its opening (1851)" = paste(
    "Call me Ishmael. Some years ago - never mind how long precisely - having little or no money in my purse,",
    "and nothing particular to interest me on shore, I thought I would sail about a little and see the watery",
    "part of the world. It is a way I have of driving off the spleen and regulating the circulation. Whenever",
    "I find myself growing grim about the mouth; whenever it is a damp, drizzly November in my soul; whenever I",
    "find myself involuntarily pausing before coffin warehouses, and bringing up the rear of every funeral I",
    "meet; and especially whenever my hypos get such an upper hand of me, that it requires a strong moral",
    "principle to prevent me from deliberately stepping into the street, and methodically knocking people's",
    "hats off - then, I account it high time to get to sea as soon as I can."
  )
)

# Keyness: the words a text uses more than the reference does, scored by
# Dunning's log-likelihood G2 (1993, Computational Linguistics 19(1)).
# Positive keyness only: words over-represented in the text.
keyness <- function(w, ref) {
  a <- table(w); b <- table(factor(ref, levels = names(a)))
  a <- as.numeric(a); b <- as.numeric(b)
  c1 <- length(w); d1 <- length(ref)
  e1 <- c1 * (a + b) / (c1 + d1); e2 <- d1 * (a + b) / (c1 + d1)
  g2 <- 2 * (a * log(a / e1) + ifelse(b > 0, b * log(b / e2), 0))
  g2[a / c1 <= b / d1] <- 0
  stats::setNames(g2, names(table(w)))
}

# Words too common to say anything about a text, left out of the bar chart
# when asked (they stay in every count).
STOP <- c("a", "an", "and", "are", "as", "at", "be", "been", "but", "by", "for", "from", "had", "has", "have",
          "he", "her", "his", "i", "if", "in", "into", "is", "it", "its", "me", "my", "no", "not", "of", "on",
          "or", "our", "she", "so", "that", "the", "their", "them", "then", "there", "these", "they", "this",
          "those", "to", "was", "we", "were", "what", "when", "which", "who", "will", "with", "would", "you")

words_of <- function(text) {
  w <- regmatches(tolower(text), gregexpr("[a-z]+(?:'[a-z]+)?", tolower(text), perl = TRUE))[[1]]
  w[nzchar(w)]
}

# Sentences: split after . ! ? followed by a space and a capital or a quote.
# Initials ("H.M.S.", "U.S.") and titles ("Dr.", "Prof.") keep their dots out of it: they are swapped for
# a placeholder while splitting, then put back.
sentences_of <- function(text) {
  t <- gsub("\\s+", " ", text)
  t <- gsub("(?<=\\b[A-Z])\\.(?=[A-Z]\\.|\\s)", "\u00b7", t, perl = TRUE)
  t <- gsub("\\b(Dr|Mr|Mrs|Ms|Prof|St|Fig|No|vs|etc|e\\.g|i\\.e)\\.", "\\1\u00b7", t, perl = TRUE)
  s <- trimws(unlist(strsplit(t, "(?<=[.!?])\\s+(?=[A-Z\"'])", perl = TRUE)))
  s <- gsub("\u00b7", ".", s, fixed = TRUE)
  s[nzchar(s)]
}

syllables <- function(w) {
  n <- lengths(regmatches(w, gregexpr("[aeiouy]+", w)))
  n <- n - (grepl("[^aeiouy]e$", w) & n > 1)
  pmax(n, 1)
}

tile <- function(value, label, tone = NULL) {
  box::use(shiny = shiny[tags])
  shiny$tags$div(class = paste("tile", tone), shiny$tags$b(value), shiny$tags$span(label))
}

card_room <- function(session, out) {
  w <- session$clientData[[paste0("output_", session$ns(out), "_width")]]
  if (!is.null(w) && w > 900) 400 / 19.2 else 1.4
}

ink <- function(mar = c(3.6, 4.4, 3, 1.2)) {
  graphics::par(bg = NA, mar = mar, col.axis = "#c3cad8", col.lab = "#dfe4ee", fg = "#5b6478",
                cex.axis = 0.95, las = 1)
}

# ── words ────────────────────────────────────────────────────────────────────

ui_words <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(shiny = shiny[NS, tags, textAreaInput, selectInput, radioButtons, sliderInput, checkboxInput, plotOutput, uiOutput])
  ns <- shiny$NS(id)
  tags$section(
    class = "stage",
    plotOutput(ns("top"), height = "100%"),
    tags$details(
      class = "stage-card glass", open = NA,
      tags$summary("Text Lab"),
      selectInput(ns("passage"), "A passage", names(PASSAGES), width = "100%"),
      textAreaInput(ns("text"), "Or your own text - paste anything", SAMPLE, rows = 7, width = "100%"),
      radioButtons(ns("rank"), NULL, c("Most frequent" = "freq", "Most distinctive" = "key"), inline = TRUE),
      sliderInput(ns("n"), "Words in the chart", 5, 40, 20, step = 1, width = "100%"),
      checkboxInput(ns("stop"), "Leave out the commonest words (the, of, and ...)", TRUE),
      uiOutput(ns("tiles"))
    )
  )
}

# Returns the text's words and sentences, for the patterns branch.
server_words <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(
    shiny = shiny[moduleServer, reactive, req, renderPlot, renderUI, tags, debounce, observeEvent, updateTextAreaInput],
    graphics[plot.new, plot.window, axis, rect, text, mtext]
  )
  moduleServer(id, function(input, output, session) {
    observeEvent(input$passage, updateTextAreaInput(session, "text", value = PASSAGES[[input$passage]]), ignoreInit = TRUE)
    typed <- debounce(reactive(input$text), 400)
    txt <- reactive({
      t <- typed()
      req(!is.null(t), nzchar(trimws(t)))
      w <- words_of(t); s <- sentences_of(t)
      list(words = w, sentences = s, per = vapply(s, function(x) length(words_of(x)), numeric(1)))
    })

    output$top <- renderPlot({
      x <- txt(); req(input$n)
      w <- if (isTRUE(input$stop)) x$words[!x$words %in% STOP] else x$words
      req(length(w) > 0)
      key <- identical(input$rank, "key")
      f <- if (key) {
        # The reference: every bundled passage that is not this text.
        others <- PASSAGES[vapply(PASSAGES, function(p) !identical(trimws(p), trimws(typed())), logical(1))]
        ref <- words_of(paste(unlist(others), collapse = " "))
        g <- keyness(w, if (isTRUE(input$stop)) ref[!ref %in% STOP] else ref)
        round(utils::head(sort(g[g > 0], decreasing = TRUE), input$n), 1)
      } else utils::head(sort(table(w), decreasing = TRUE), input$n)
      req(length(f) > 0)
      k <- length(f)
      ink(c(3.4, 9, 3.4, card_room(session, "top")))
      plot.new(); plot.window(xlim = c(0, max(f) * 1.1), ylim = c(0.4, k + 0.6), xaxs = "i", yaxs = "i")
      axis(1, at = pretty(c(0, max(f))))
      y <- rev(seq_len(k))
      rect(0, y - 0.38, f, y + 0.38, col = if (key) "#F0DC7859" else "#5AC8FA59", border = if (key) TXT$col[["fit"]] else TXT$col[["bar"]])
      text(0, y, names(f), pos = 2, xpd = NA, col = "#e6e9ef", cex = 1.05)
      text(f, y, f, pos = 4, col = "#c3cad8", cex = 0.9)
      mtext_fit(if (key) sprintf("the %d most distinctive words, against the other passages", k)
            else sprintf("the %d most frequent words%s", k, if (isTRUE(input$stop)) ", the commonest left out" else ""),
            side = 3, line = 1.4, adj = 0, col = TXT$col[["ink"]], cex = 1.1, font = 2)
      mtext_fit(if (key) "keyness, log-likelihood G2" else "times used", side = 1, line = 2.3, col = "#c3cad8")
    }, bg = "transparent", res = 96)

    output$tiles <- renderUI({
      x <- txt()
      n <- length(x$words); ns <- max(length(x$sentences), 1)
      ease <- 206.835 - 1.015 * n / ns - 84.6 * sum(syllables(x$words)) / max(n, 1)
      tags$div(
        class = "tiles",
        tile(format(n, big.mark = ","), "words"),
        tile(format(length(unique(x$words)), big.mark = ","), "different words"),
        tile(length(x$sentences), "sentences"),
        tile(formatC(n / ns, format = "f", digits = 1), "words a sentence, on average"),
        tile(formatC(ease, format = "f", digits = 0), "reading ease (Flesch, 100 is easiest)",
             if (ease >= 60) "tone-good" else if (ease < 30) "tone-bad" else NULL),
        tile(paste0(round(100 * length(unique(x$words)) / max(n, 1)), "%"), "different words per word used")
      )
    })

    txt
  })
}

# ── patterns ─────────────────────────────────────────────────────────────────

ui_patterns <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(shiny = shiny[NS, tags, plotOutput])
  ns <- shiny$NS(id)
  tags$section(
    class = "stage-duo",
    tags$div(class = "glass", plotOutput(ns("lengths"), height = "100%")),
    tags$div(class = "glass", plotOutput(ns("zipf"), height = "100%"))
  )
}

server_patterns <- function(id, txt) {
  box::use(./r/log)
  log$log_fn()
  box::use(
    shiny = shiny[moduleServer, renderPlot, req],
    graphics[plot.new, plot.window, axis, rect, lines, points, abline, mtext, hist]
  )
  moduleServer(id, function(input, output, session) {
    output$lengths <- renderPlot({
      x <- txt()
      req(length(x$per) > 0)
      h <- hist(x$per, breaks = seq(0, max(x$per) + 5, by = 5), plot = FALSE)
      ink(c(3.6, 4.6, 4.2, 1.2))
      plot.new(); plot.window(xlim = range(h$breaks), ylim = c(0, max(h$counts) * 1.1), xaxs = "i", yaxs = "i")
      axis(1); axis(2)
      rect(h$breaks[-length(h$breaks)], 0, h$breaks[-1], h$counts, col = "#3DDC9759", border = TXT$col[["hist"]])
      abline(v = mean(x$per), col = "#ffffff", lwd = 2, lty = 2)
      mtext_fit("how long the sentences run", side = 3, line = 2.2, adj = 0, col = TXT$col[["ink"]], cex = 1.1, font = 2)
      mtext_fit(sprintf("words in each sentence; dashed: the mean, %.1f", mean(x$per)), side = 3, line = 0.7, adj = 0, col = "#dfe4ee", cex = 0.95)
      mtext_fit("words in the sentence", side = 1, line = 2.4, col = "#c3cad8")
    }, bg = "transparent", res = 96)

    output$zipf <- renderPlot({
      x <- txt()
      f <- as.numeric(sort(table(x$words), decreasing = TRUE))
      req(length(f) >= 3)
      r <- seq_along(f)
      fit <- stats::lm(log10(f) ~ log10(r))
      ink(c(3.6, 4.6, 4.2, 1.2))
      plot.new(); plot.window(xlim = range(r), ylim = range(f), log = "xy")
      axis(1); axis(2)
      points(r, f, pch = 19, cex = 0.8, col = "#5AC8FAAA")
      lines(r, 10^stats::predict(fit), col = TXT$col[["fit"]], lwd = 2.4)
      mtext_fit("Zipf's law: frequency against rank", side = 3, line = 2.2, adj = 0, col = TXT$col[["ink"]], cex = 1.1, font = 2)
      mtext_fit(sprintf("log-log; the fitted slope is %.2f (Zipf: about -1, clearer in longer texts)", stats::coef(fit)[2]),
            side = 3, line = 0.7, adj = 0, col = "#dfe4ee", cex = 0.95)
      mtext_fit("rank of the word", side = 1, line = 2.4, col = "#c3cad8")
    }, bg = "transparent", res = 96)
  })
}

# ── the site ─────────────────────────────────────────────────────────────────

# The stages: a visual filling the window under the site's header, its
# controls in a glass card that folds to its title (a <details>: no script),
# top right on a desk and along the bottom on a phone.
STAGE_CSS <- "
.stage { position: relative; height: calc(100vh - 56px); min-height: 520px; overflow: hidden;
  background: radial-gradient(ellipse at 50% 35%, #16203d 0%, #090c16 55%, #04050a 100%); }
.stage > .shiny-plot-output { position: absolute; inset: 0; }
.glass { background: rgba(10, 13, 22, 0.80); border: 1px solid rgba(255, 255, 255, 0.10);
  border-radius: 14px; backdrop-filter: blur(8px); color: #e6e9ef; }
.stage-card { position: absolute; top: 16px; right: 16px; width: 360px; max-height: calc(100% - 32px);
  overflow: auto; padding: 12px 16px; }
.stage-card summary { font-size: 1.2rem; font-weight: 600; cursor: pointer; }
.stage-card[open] summary { margin-bottom: 8px; }
.stage-duo { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; padding: 16px;
  height: calc(100vh - 56px); min-height: 520px; background: linear-gradient(180deg, #04050a 0%, #0b1022 100%); }
.stage-duo > .glass { position: relative; padding: 8px; min-height: 0; }
.stage-duo .shiny-plot-output { position: absolute; inset: 8px; height: auto !important; }
.tiles { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; margin-top: 10px; }
.tile { background: rgba(255, 255, 255, 0.06); border-radius: 10px; padding: 7px 10px; }
.tile b { display: block; font-size: 1.35rem; font-weight: 600; color: #fff; font-variant-numeric: tabular-nums; }
.tile span { font-size: 0.9rem; color: #d3d9e6; }
.tile.tone-good b { color: #3DDC97; }
.tile.tone-bad b { color: #FF7A7E; }
@media (max-width: 760px) {
  .stage-card { top: auto; bottom: 12px; left: 12px; right: 12px; width: auto; max-height: 46%; }
  .stage:has(.stage-card[open]) > .shiny-plot-output { bottom: 48%; }
  .stage-duo { grid-template-columns: 1fr; height: auto; padding: 10px; gap: 10px; }
  .stage-duo > .glass { height: 360px; }
}
"

# On a phone the cards start folded: the visual gets the whole screen.
STAGE_JS <- "document.addEventListener('DOMContentLoaded', function () {
  if (window.innerWidth < 760) document.querySelectorAll('.stage-card').forEach(function (d) { d.open = false; });
});"


# A title or axis label that fits its figure on a phone (2026-10-27: on a
# 390 px screen "the 20 most frequent words" ran past the plot's edge). When
# the text is wider than the room from where it is anchored, it first moves -
# a left-aligned title to the figure's left edge, a centred one to the
# figure's centre - and only then shrinks. On a wide plot nothing changes.
mtext_fit <- function(text, side = 3, line = 0, adj = 0.5, cex = 1, font = 1, at = NA, ...) {
  if (side %in% c(1, 3) && is.na(at[1])) {
    usr <- graphics::par("usr")
    lo <- graphics::grconvertX(0, "nfc", "user"); hi <- graphics::grconvertX(1, "nfc", "user")
    pad <- 0.02 * (hi - lo)
    anchor <- usr[1] + adj * diff(usr[1:2])
    room <- if (adj <= 0) hi - pad - anchor else if (adj >= 1) anchor - lo - pad else 2 * min(anchor - lo, hi - anchor) - 2 * pad
    w <- max(graphics::strwidth(text, cex = cex, font = font))
    if (is.finite(w) && w > room) {
      if (adj <= 0) { at <- lo + pad; room <- hi - lo - 2 * pad }
      else if (adj >= 1) { at <- hi - pad; room <- hi - lo - 2 * pad }
      else { at <- (lo + hi) / 2; room <- hi - lo - 2 * pad }
      if (w > room) cex <- cex * room / w
    }
  }
  graphics::mtext(text, side = side, line = line, adj = adj, cex = cex, font = font, at = at, ...)
}

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
    body = shiny$tagList(
      # HTML(): as plain text the child selectors (">") would be escaped to &gt; and dropped.
      shiny$tags$style(shiny$HTML(STAGE_CSS)),
      shiny$tags$script(shiny$HTML(STAGE_JS)),
      ui_words(ns("words")),
      ui_patterns(ns("patterns"))
    )
  )
}

.server_inner <- function(id = "__HOST__", parent_session) {
  box::use(r / shell / shell, ./r/log)
  shell$server_shell(id, parent_session, inner = function(input, output, session) {
    log$log_session(session)
    log$log_info("__HOST__: module server starting")
    x <- server_words("words")
    server_patterns("patterns", x)
  })
}

#' @export
server_module <- local({
  box::use(r / observability / timing[measured])
  wrapped <- measured("__HOST__", .server_inner)
  function(id = "__HOST__", ...) wrapped(id, ...)
})
