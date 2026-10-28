// Project Canvas, the page: React 18 components written with htm, no compile step.
//
// State is two things: the last board the server confirmed, and the queue of
// operations this tab has sent but not yet seen confirmed. What you see is the
// queue applied on top of the confirmed board, with the same BoardOps.apply the
// server runs. A drag shows at once (optimistic); when the server answers, the
// operation leaves the queue and its board becomes the confirmed one; when it
// refuses, the operation leaves the queue and the card falls back by itself.
// Other tabs' changes arrive over /api/events and replace the confirmed board
// underneath the queue, so neither side's move is lost.
"use strict";

const { useState, useEffect, useMemo, useReducer, useRef, useCallback } = React;
const html = htm.bind(React.createElement);

const ME = sessionStorage.getItem("canvas-client") || BoardOps.newId();
sessionStorage.setItem("canvas-client", ME);

// ---- state -----------------------------------------------------------------
const initial = { confirmed: null, pending: [], live: { status: "connecting", clients: 0 }, toasts: [], flash: {} };

function newer(a, b) { return !a || (b && b.version > a.version) ? b : a; }

function reducer(s, a) {
  switch (a.type) {
    case "board": return { ...s, confirmed: newer(s.confirmed, a.board) };
    case "send": return { ...s, pending: [...s.pending, { key: a.key, op: a.op }] };
    case "settle": return { ...s, pending: s.pending.filter((p) => p.key !== a.key), confirmed: newer(s.confirmed, a.board) };
    case "live": return { ...s, live: { ...s.live, ...a.live } };
    case "toast": return { ...s, toasts: [...s.toasts.slice(-2), { id: a.id, text: a.text, bad: a.bad }] };
    case "untoast": return { ...s, toasts: s.toasts.filter((t) => t.id !== a.id) };
    case "flash": return { ...s, flash: { ...s.flash, [a.id]: a.on ? Date.now() : undefined } };
    default: return s;
  }
}

function view(confirmed, pending) {
  // An operation that no longer applies (another tab deleted its card) is
  // skipped here; the server will refuse it too and it leaves the queue.
  return pending.reduce((b, p) => { try { return BoardOps.apply(b, p.op); } catch { return b; } }, confirmed);
}

// One request per operation; the server answers with the event it broadcast.
function request(op) {
  const headers = { "Content-Type": "application/json", "X-Client-Id": ME };
  const call = (method, url, body) => fetch(url, { method, headers, body: body && JSON.stringify(body) })
    .then(async (r) => { const j = await r.json().catch(() => ({})); if (!r.ok) throw new Error(j.error || r.statusText); return j; });
  switch (op.type) {
    case "create": return call("POST", "/api/cards", { ...op.card, column: op.column, before: op.before });
    case "update": return call("PATCH", "/api/cards/" + op.id, op.fields);
    case "move": return call("PATCH", "/api/cards/" + op.id, { column: op.column, before: op.before });
    case "delete": return call("DELETE", "/api/cards/" + op.id);
    case "reset": return call("POST", "/api/board/reset");
  }
}

function useBoard() {
  const [s, dispatch] = useReducer(reducer, initial);
  const board = useMemo(() => s.confirmed && view(s.confirmed, s.pending), [s.confirmed, s.pending]);

  const toast = useCallback((text, bad) => {
    const id = Math.random();
    dispatch({ type: "toast", id, text, bad });
    setTimeout(() => dispatch({ type: "untoast", id }), 3200);
  }, []);
  const flash = useCallback((id) => {
    dispatch({ type: "flash", id, on: true });
    setTimeout(() => dispatch({ type: "flash", id, on: false }), 1600);
  }, []);

  const send = useCallback((op) => {
    const key = Math.random();
    if (op.type !== "reset") dispatch({ type: "send", key, op });
    request(op).then(
      (ev) => dispatch({ type: "settle", key, board: ev.board }),
      (e) => { dispatch({ type: "settle", key }); toast("Not saved: " + e.message, true); });
  }, [toast]);

  // Live stream. EventSource reconnects by itself; hello carries the board.
  useEffect(() => {
    const es = new EventSource("/api/events?client=" + encodeURIComponent(ME));
    es.addEventListener("hello", (e) => {
      const d = JSON.parse(e.data);
      dispatch({ type: "board", board: d.board });
      dispatch({ type: "live", live: { status: "live", clients: d.clients } });
    });
    es.addEventListener("presence", (e) => dispatch({ type: "live", live: { clients: JSON.parse(e.data).clients } }));
    es.addEventListener("change", (e) => {
      const d = JSON.parse(e.data);
      dispatch({ type: "board", board: d.board });
      if (d.by === ME) return;
      const card = d.op.id && d.board.cards[d.op.id];
      const col = d.board.columns.find((c) => c.id === d.op.column);
      if (card) flash(card.id);
      const what = { move: `moved “${card?.title}” to ${col?.title}`, update: `edited “${card?.title}”`, create: `added “${card?.title}”`, delete: "deleted a card", replace: "reset the board" }[d.op.type];
      if (what) toast((d.by === "api" ? "API " : "Another tab ") + what);
    });
    es.onerror = () => dispatch({ type: "live", live: { status: "reconnecting" } });
    es.onopen = () => dispatch({ type: "live", live: { status: "live" } });
    return () => es.close();
  }, [flash, toast]);

  return { board, s, send, toast };
}

// ---- helpers ---------------------------------------------------------------
const OWNER_COLORS = ["#14a0be", "#d6702a", "#9a7be6", "#3fae74", "#d4598f", "#c7a33a"];
const hash = (s) => [...s].reduce((h, c) => (h * 31 + c.charCodeAt(0)) >>> 0, 7);
const ownerColor = (o) => OWNER_COLORS[hash(o || "?") % OWNER_COLORS.length];
const initials = (o) => (o || "?").split(/\s+/).map((w) => w[0]).join("").slice(0, 2).toUpperCase();
const COARSE = matchMedia("(pointer: coarse)").matches;
const today = () => new Date().toISOString().slice(0, 10);
const shortDate = (d) => new Date(d + "T12:00:00").toLocaleDateString(undefined, { month: "short", day: "numeric" });

function dueState(card, doneCol) {
  if (!card.due || doneCol) return "";
  const days = Math.round((new Date(card.due + "T12:00:00") - new Date(today() + "T12:00:00")) / 864e5);
  return days < 0 ? "late" : days <= 2 ? "soon" : "";
}

// Where a drop lands: in front of the first card whose middle is below the
// pointer, else at the end. The dragged card itself is not a neighbour.
function beforeAt(colEl, y, dragId) {
  for (const el of colEl.querySelectorAll("[data-card]")) {
    if (el.dataset.card === dragId) continue;
    const r = el.getBoundingClientRect();
    if (y < r.top + r.height / 2) return el.dataset.card;
  }
  return null;
}

// ---- drag and drop (mouse: HTML5; touch: long-press) ----------------------
function useDrag(board, send) {
  const [drag, setDrag] = useState(null); // { id, column, before }
  const ref = useRef({ drag: null, timer: 0, start: null, ghost: null, justDragged: false });
  ref.current.drag = drag;
  ref.current.board = board;

  const drop = useCallback((d) => {
    if (!d || !d.column) return;
    const b = ref.current.board;
    const from = BoardOps.columnOf(b, d.id);
    if (!from) return; // deleted in another tab mid-drag
    const list = from.cards, i = list.indexOf(d.id);
    const unchanged = from.id === d.column && (d.before === list[i + 1] || (d.before == null && i === list.length - 1));
    if (!unchanged) send({ type: "move", id: d.id, column: d.column, before: d.before });
  }, [send]);

  const over = useCallback((colEl, y) => {
    const d = ref.current.drag;
    if (!d) return;
    const column = colEl.dataset.column, before = beforeAt(colEl, y, d.id);
    if (d.column !== column || d.before !== before) setDrag({ ...d, column, before });
  }, []);

  // Touch: hold still 220 ms to pick a card up; moving first means scrolling.
  useEffect(() => {
    const r = ref.current;
    const move = (e) => {
      const t = e.touches[0];
      if (!r.drag) {
        if (r.start && Math.hypot(t.clientX - r.start.x, t.clientY - r.start.y) > 8) { clearTimeout(r.timer); r.start = null; }
        return;
      }
      e.preventDefault(); // the page must not scroll under a dragged card
      if (r.ghost) r.ghost.style.transform = `translate(${t.clientX - 40}px, ${t.clientY - 24}px)`;
      const el = document.elementFromPoint(t.clientX, t.clientY);
      const col = el && el.closest("[data-column]");
      if (col) over(col, t.clientY);
      // Near a side edge on a phone the board scrolls to the next column.
      const scroller = document.querySelector(".columns");
      if (scroller) {
        if (t.clientX < 40) scroller.scrollBy({ left: -18 });
        else if (t.clientX > innerWidth - 40) scroller.scrollBy({ left: 18 });
      }
    };
    const end = () => {
      clearTimeout(r.timer);
      r.start = null;
      if (r.drag) {
        drop(r.drag);
        r.justDragged = true;
        setTimeout(() => (r.justDragged = false), 50);
        setDrag(null);
      }
    };
    addEventListener("touchmove", move, { passive: false });
    addEventListener("touchend", end);
    addEventListener("touchcancel", end);
    return () => { removeEventListener("touchmove", move); removeEventListener("touchend", end); removeEventListener("touchcancel", end); };
  }, [drop, over]);

  // Touch screens get the long-press path only; a native drag starting as
  // well would move the card twice.
  const cardProps = (card) => ({
    draggable: !COARSE,
    onContextMenu: (e) => ref.current.drag && e.preventDefault(),
    onDragStart: (e) => {
      e.dataTransfer.effectAllowed = "move";
      e.dataTransfer.setData("text/plain", card.id);
      setTimeout(() => setDrag({ id: card.id, column: null, before: null }), 0);
    },
    onDragEnd: () => setDrag(null),
    onTouchStart: (e) => {
      const t = e.touches[0], r = ref.current;
      r.start = { x: t.clientX, y: t.clientY };
      clearTimeout(r.timer);
      r.timer = setTimeout(() => {
        navigator.vibrate?.(12);
        setDrag({ id: card.id, column: null, before: null, touch: { x: t.clientX, y: t.clientY, title: card.title } });
      }, 220);
    },
  });

  const columnProps = (colId) => ({
    "data-column": colId,
    onDragOver: (e) => { if (!ref.current.drag) return; e.preventDefault(); over(e.currentTarget, e.clientY); },
    onDrop: (e) => { e.preventDefault(); drop(ref.current.drag); setDrag(null); },
  });

  return { drag, cardProps, columnProps, ghostRef: (el) => (ref.current.ghost = el), justDragged: () => ref.current.justDragged };
}

// ---- components ------------------------------------------------------------
function Avatar({ owner, size = 26 }) {
  return html`<span class="avatar" title=${owner || "unassigned"} style=${{ background: ownerColor(owner), width: size, height: size, fontSize: size * 0.42 }}>${initials(owner)}</span>`;
}

function Card({ card, doneCol, dragging, flashing, onOpen, dragProps }) {
  const doneItems = card.checklist.filter((i) => i.done).length, total = card.checklist.length;
  const due = dueState(card, doneCol);
  return html`
    <article class=${"card" + (dragging ? " dragging" : "") + (flashing ? " flash" : "")} data-card=${card.id}
      tabindex="0" onClick=${onOpen} onKeyDown=${(e) => e.key === "Enter" && onOpen()} ...${dragProps}>
      <h3>${card.title}</h3>
      ${card.tags.length > 0 && html`<div class="tags">${card.tags.map((t) => html`<span key=${t} class="tag">${t}</span>`)}</div>`}
      ${total > 0 && html`<div class="progress" title=${`${doneItems} of ${total} steps done`}>
        <span class="bar"><i style=${{ width: (100 * doneItems) / total + "%" }}></i></span><small>${doneItems}/${total}</small></div>`}
      <footer>
        <${Avatar} owner=${card.owner} size=${24} />
        <span class="owner">${card.owner || "unassigned"}</span>
        ${card.due && !doneCol && html`<span class=${"due " + due}>${due === "late" ? "overdue · " : ""}${shortDate(card.due)}</span>`}
        ${doneCol && card.done && html`<span class="due done">✓ ${shortDate(card.done.slice(0, 10))}</span>`}
      </footer>
    </article>`;
}

function Column({ col, board, visible, drag, d, flash, onOpen, onAdd, isDone }) {
  const cards = col.cards.filter((id) => visible.has(id)).map((id) => board.cards[id]);
  const line = html`<div key="drop-line" class="drop-line"></div>`;
  const items = [];
  for (const c of cards) {
    if (drag && drag.column === col.id && drag.before === c.id) items.push(line);
    items.push(html`<${Card} key=${c.id} card=${c} doneCol=${isDone} dragging=${drag && drag.id === c.id}
      flashing=${!!flash[c.id]} onOpen=${() => !d.justDragged() && onOpen(c.id)} dragProps=${d.cardProps(c)} />`);
  }
  if (drag && drag.column === col.id && drag.before == null) items.push(line);
  return html`
    <section class=${"column glass" + (drag && drag.column === col.id ? " over" : "")} ...${d.columnProps(col.id)}>
      <header><h2>${col.title}</h2><span class="count">${cards.length}${cards.length !== col.cards.length ? ` / ${col.cards.length}` : ""}</span>
        <button class="add" title=${"Add a card to " + col.title} onClick=${() => onAdd(col.id)}>+</button></header>
      <div class="cards">${items}${!cards.length && !drag && html`<p class="empty">No cards</p>`}</div>
    </section>`;
}

function Filters({ board, filter, setFilter }) {
  const all = Object.values(board.cards);
  const owners = [...new Set(all.map((c) => c.owner).filter(Boolean))].sort();
  const tags = [...new Set(all.flatMap((c) => c.tags))].sort();
  const toggleTag = (t) => setFilter((f) => { const s = new Set(f.tags); s.has(t) ? s.delete(t) : s.add(t); return { ...f, tags: s }; });
  const any = filter.owner || filter.tags.size || filter.q;
  return html`
    <div class="filters">
      <input class="search" type="search" placeholder="Search cards" value=${filter.q} onInput=${(e) => setFilter((f) => ({ ...f, q: e.target.value }))} />
      <div class="chips owners">${owners.map((o) => html`
        <button key=${o} class=${"chip" + (filter.owner === o ? " on" : "")} onClick=${() => setFilter((f) => ({ ...f, owner: f.owner === o ? null : o }))}>
          <${Avatar} owner=${o} size=${20} />${o}</button>`)}</div>
      <div class="chips">${tags.map((t) => html`
        <button key=${t} class=${"chip tagchip" + (filter.tags.has(t) ? " on" : "")} onClick=${() => toggleTag(t)}>#${t}</button>`)}</div>
      ${any && html`<button class="chip clear" onClick=${() => setFilter({ owner: null, tags: new Set(), q: "" })}>Clear filters</button>`}
    </div>`;
}

// Burndown (open cards at the end of each day) over throughput (cards that
// reached the last column that day). Two charts on one time axis, never two
// scales on one chart. Both follow the filters.
function Chart({ board, visible }) {
  const [hover, setHover] = useState(null);
  const DAYS = 21, W = 320, H1 = 96, H2 = 46, GAP = 22, L = 26, R = 8, T = 8;
  const data = useMemo(() => {
    const cards = Object.values(board.cards).filter((c) => visible.has(c.id));
    const end0 = new Date(); end0.setHours(24, 0, 0, 0);
    return Array.from({ length: DAYS }, (_, i) => {
      const end = new Date(end0 - (DAYS - 1 - i) * 864e5), start = new Date(end - 864e5);
      const open = cards.filter((c) => new Date(c.created) < end && (!c.done || new Date(c.done) >= end)).length;
      const done = cards.filter((c) => c.done && new Date(c.done) >= start && new Date(c.done) < end).length;
      return { day: start, open, done };
    });
  }, [board, visible]);
  const maxOpen = Math.max(4, ...data.map((d) => d.open)), maxDone = Math.max(2, ...data.map((d) => d.done));
  const step = (W - L - R) / DAYS, x = (i) => L + step * (i + 0.5);
  const y1 = (v) => T + H1 - (v / maxOpen) * H1, y2base = T + H1 + GAP + H2;
  const linePath = data.map((d, i) => `${i ? "L" : "M"}${x(i).toFixed(1)},${y1(d.open).toFixed(1)}`).join("");
  const area = `${linePath}L${x(DAYS - 1)},${T + H1}L${x(0)},${T + H1}Z`;
  const last = data[DAYS - 1], doneTotal = data.reduce((a, d) => a + d.done, 0);
  const h = hover != null ? data[hover] : null;
  const fmt = (d) => d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
  const onMove = (e) => {
    const r = e.currentTarget.getBoundingClientRect();
    const px = ((e.clientX ?? e.touches?.[0]?.clientX) - r.left) * (W / r.width);
    setHover(Math.max(0, Math.min(DAYS - 1, Math.floor((px - L) / step))));
  };
  return html`
    <section class="glass chart">
      <header>
        <div><b class="big">${last.open}</b><span class="k">open</span></div>
        <div><b class="big">${doneTotal}</b><span class="k">written up · ${DAYS} d</span></div>
      </header>
      <svg viewBox=${`0 0 ${W} ${y2base + 18}`} role="img" aria-label=${`Open cards over ${DAYS} days, ending at ${last.open}; ${doneTotal} cards written up`}
        onMouseMove=${onMove} onMouseLeave=${() => setHover(null)} onTouchStart=${onMove} onTouchMove=${onMove}>
        <text x=${L} y=${T - 1} class="axis-title">Open cards</text>
        ${[0, maxOpen].map((v) => html`<g key=${"o" + v}><line x1=${L} x2=${W - R} y1=${y1(v)} y2=${y1(v)} class="grid" /><text x=${L - 5} y=${y1(v) + 3} class="tick" text-anchor="end">${v}</text></g>`)}
        <path d=${area} class="area" />
        <path d=${linePath} class="line" />
        <circle cx=${x(DAYS - 1)} cy=${y1(last.open)} r="4" class="dot" />
        <text x=${L} y=${T + H1 + GAP - 6} class="axis-title">Written up per day</text>
        <line x1=${L} x2=${W - R} y1=${y2base} y2=${y2base} class="grid" />
        <text x=${L - 5} y=${y2base - H2 + 3} class="tick" text-anchor="end">${maxDone}</text>
        ${data.map((d, i) => d.done > 0 && html`<rect key=${"b" + i} x=${x(i) - step * 0.32} width=${step * 0.64}
          y=${y2base - (d.done / maxDone) * H2} height=${(d.done / maxDone) * H2} rx="2" class="bar" />`)}
        <text x=${x(0)} y=${y2base + 14} class="tick" text-anchor="start">${fmt(data[0].day)}</text>
        <text x=${x(DAYS - 1)} y=${y2base + 14} class="tick" text-anchor="end">today</text>
        ${h && html`<g class="hover">
          <line x1=${x(hover)} x2=${x(hover)} y1=${T} y2=${y2base} class="cross" />
          <circle cx=${x(hover)} cy=${y1(h.open)} r="4" class="dot" />
        </g>`}
      </svg>
      <p class="readout">${h ? html`<b>${fmt(h.day)}</b> · ${h.open} open · ${h.done} written up` : html`Hover or touch the chart for a day`}</p>
    </section>`;
}

// Read live from GET /api/env each time it opens: what runs, from which build.
function Environment({ clients }) {
  const [env, setEnv] = useState(null), [open, setOpen] = useState(false);
  useEffect(() => { if (open) fetch("/api/env").then((r) => r.json()).then(setEnv).catch(() => setEnv({ error: true })); }, [open, clients]);
  const row = (k, v) => html`<div class="env-row" key=${k}><span class="k">${k}</span><span class="v">${v}</span></div>`;
  return html`
    <details class="glass env" onToggle=${(e) => setOpen(e.currentTarget.open)}>
      <summary>Environment</summary>
      ${open && !env && html`<p class="k">reading /api/env…</p>`}
      ${env && env.error && html`<p class="k">/api/env did not answer</p>`}
      ${env && !env.error && html`<div class="env-body">
        ${row("runtime", `${env.runtime.name} ${env.runtime.version} · V8 ${env.runtime.v8} · ${env.runtime.platform}`)}
        ${row("stack", [...env.stack.env, ...env.stack.runtime].join(" · ") || "no stack module loaded (node from PATH)")}
        ${row("server", env.server.map((m) => m.name).join(", ") + ` (built in, ${env.runtime.version})`)}
        ${env.browser.map((b) => row(b.name, html`${b.version} <span class=${b.verified ? "ok" : "bad"}>${b.verified ? "✓ sha256" : "✗ checksum"}</span> <code>${(b.sha256 || "").slice(0, 12)}</code>`))}
        ${row("in this tab", `React ${React.version}`)}
        ${row("started", new Date(env.process.started).toLocaleString() + ` · pid ${env.process.pid}`)}
        ${row("host", `${env.host.cpus} CPUs · load ${env.host.load.join(" ")} · ${env.host.memory_gb} GB`)}
        ${row("live", `${env.live.sse_clients} open streams · board v${env.live.board_version} · ${env.live.board_file}`)}
      </div>`}
    </details>`;
}

function Editor({ board, editing, onClose, send }) {
  const isNew = !editing.id;
  const card = isNew ? null : board.cards[editing.id];
  const startCol = isNew ? editing.column : BoardOps.columnOf(board, editing.id)?.id;
  const [draft, setDraft] = useState(() => ({
    title: card?.title || "", owner: card?.owner || "", tags: (card?.tags || []).join(", "), due: card?.due || "",
    checklist: card ? card.checklist.map((i) => ({ ...i })) : [], column: startCol, item: "",
  }));
  const owners = [...new Set(Object.values(board.cards).map((c) => c.owner).filter(Boolean))].sort();
  useEffect(() => { const k = (e) => e.key === "Escape" && onClose(); addEventListener("keydown", k); return () => removeEventListener("keydown", k); }, [onClose]);
  const gone = !isNew && !card; // deleted in another tab while open
  useEffect(() => { if (gone) onClose(); }, [gone, onClose]);
  if (gone) return null;

  const set = (k) => (e) => setDraft((d) => ({ ...d, [k]: e.target.type === "checkbox" ? e.target.checked : e.target.value }));
  const fields = () => ({ title: draft.title, owner: draft.owner, tags: draft.tags.split(","), due: draft.due || null, checklist: draft.checklist });
  const addItem = () => draft.item.trim() && setDraft((d) => ({ ...d, checklist: [...d.checklist, { text: d.item.trim(), done: false }], item: "" }));
  const save = (e) => {
    e.preventDefault();
    if (isNew) send({ type: "create", card: { id: BoardOps.newId(), ...fields() }, column: draft.column, before: null });
    else {
      send({ type: "update", id: card.id, fields: fields() });
      if (draft.column !== startCol) send({ type: "move", id: card.id, column: draft.column, before: null });
    }
    onClose();
  };
  const remove = () => { if (confirm(`Delete “${card.title}”?`)) { send({ type: "delete", id: card.id }); onClose(); } };

  return html`
    <div class="scrim" onClick=${(e) => e.target === e.currentTarget && onClose()}>
      <form class="glass editor" onSubmit=${save}>
        <input class="title" value=${draft.title} onInput=${set("title")} placeholder="What needs doing" required autoFocus=${isNew} />
        <div class="grid2">
          <label>Owner<input list="owners" value=${draft.owner} onInput=${set("owner")} placeholder="unassigned" />
            <datalist id="owners">${owners.map((o) => html`<option key=${o} value=${o} />`)}</datalist></label>
          <label>Due<input type="date" value=${draft.due} onInput=${set("due")} /></label>
          <label>Column<select value=${draft.column} onChange=${set("column")}>
            ${board.columns.map((c) => html`<option key=${c.id} value=${c.id}>${c.title}</option>`)}</select></label>
          <label>Tags<input value=${draft.tags} onInput=${set("tags")} placeholder="qpcr, culture" /></label>
        </div>
        <fieldset class="checklist"><legend>Checklist</legend>
          ${draft.checklist.map((it, i) => html`
            <div class="item" key=${i}>
              <input type="checkbox" checked=${it.done} onChange=${(e) => setDraft((d) => ({ ...d, checklist: d.checklist.map((x, j) => (j === i ? { ...x, done: e.target.checked } : x)) }))} />
              <input class="text" value=${it.text} onInput=${(e) => setDraft((d) => ({ ...d, checklist: d.checklist.map((x, j) => (j === i ? { ...x, text: e.target.value } : x)) }))} />
              <button type="button" class="x" title="Remove step" onClick=${() => setDraft((d) => ({ ...d, checklist: d.checklist.filter((_, j) => j !== i) }))}>×</button>
            </div>`)}
          <div class="item new"><input class="text" value=${draft.item} onInput=${set("item")} placeholder="Add a step, Enter"
            onKeyDown=${(e) => { if (e.key === "Enter") { e.preventDefault(); addItem(); } }} /><button type="button" onClick=${addItem}>Add</button></div>
        </fieldset>
        <div class="buttons">
          ${!isNew && html`<button type="button" class="danger" onClick=${remove}>Delete</button>`}
          <span class="spacer"></span>
          <button type="button" onClick=${onClose}>Cancel</button>
          <button class="primary" type="submit">${isNew ? "Add card" : "Save"}</button>
        </div>
      </form>
    </div>`;
}

function App() {
  const { board, s, send } = useBoard();
  const [filter, setFilter] = useState({ owner: null, tags: new Set(), q: "" });
  const [editing, setEditing] = useState(null);
  const [showFilters, setShowFilters] = useState(false);
  const d = useDrag(board, send);

  const visible = useMemo(() => {
    if (!board) return new Set();
    const q = filter.q.trim().toLowerCase();
    return new Set(Object.values(board.cards).filter((c) =>
      (!filter.owner || c.owner === filter.owner) &&
      (!filter.tags.size || c.tags.some((t) => filter.tags.has(t))) &&
      (!q || c.title.toLowerCase().includes(q))).map((c) => c.id));
  }, [board, filter]);

  if (!board) return html`<div class="loading">Connecting to the board…</div>`;
  const lastCol = board.columns[board.columns.length - 1].id;
  const live = s.live;
  const touch = d.drag && d.drag.touch;
  const filtered = filter.owner || filter.tags.size || filter.q;

  return html`
    <div class="app">
      <header class="top glass">
        <div class="brand">
          <h1>Project Canvas</h1>
          <p>${board.title}</p>
        </div>
        <div class=${"live " + live.status} title="Server-Sent Events from /api/events">
          <i></i>${live.status === "live" ? `live · ${live.clients} ${live.clients === 1 ? "tab" : "tabs"}` : live.status}
          ${s.pending.length > 0 && html`<span class="pending">saving ${s.pending.length}</span>`}
        </div>
        <div class="actions">
          <button class=${"fold" + (filtered ? " on" : "")} onClick=${() => setShowFilters((v) => !v)}>Filter${filtered ? " ●" : ""}</button>
          <button onClick=${() => confirm("Replace the board with the sample board, in every open tab?") && send({ type: "reset" })}>Reset sample</button>
          <button class="primary" onClick=${() => setEditing({ column: board.columns[0].id })}>+ Card</button>
        </div>
        <div class=${"filter-row" + (showFilters ? " open" : "")}><${Filters} board=${board} filter=${filter} setFilter=${setFilter} /></div>
      </header>

      <main class="body">
        <div class="columns">
          ${board.columns.map((col) => html`<${Column} key=${col.id} col=${col} board=${board} visible=${visible} drag=${d.drag} d=${d}
            flash=${s.flash} isDone=${col.id === lastCol} onOpen=${(id) => setEditing({ id })} onAdd=${(column) => setEditing({ column })} />`)}
        </div>
        <aside class="side">
          <${Chart} board=${board} visible=${visible} />
          <${Environment} clients=${live.clients} />
        </aside>
      </main>

      ${touch && html`<div class="ghost" ref=${d.ghostRef} style=${{ transform: `translate(${touch.x - 40}px, ${touch.y - 24}px)` }}>${touch.title}</div>`}
      ${editing && html`<${Editor} key=${editing.id || "new"} board=${board} editing=${editing} send=${send} onClose=${() => setEditing(null)} />`}
      <div class="toasts">${s.toasts.map((t) => html`<div key=${t.id} class=${"toast glass" + (t.bad ? " bad" : "")}>${t.text}</div>`)}</div>
    </div>`;
}

ReactDOM.createRoot(document.getElementById("root")).render(html`<${App} />`);
