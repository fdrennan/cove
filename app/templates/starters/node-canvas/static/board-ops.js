// The board's data structure and every change to it, in one file that both
// sides run: server.js requires it, the page loads it with a <script> tag. The
// browser applies an operation at once (optimistic) and the server applies the
// same operation to its copy, so the two cannot drift apart by implementation.
//
// Board: { version, updated, columns: [{ id, title, cards: [cardId…] }],
//          cards: { [id]: { id, title, owner, tags, due, checklist, created, done } } }
// A card's place is its id in one column's ordered list, nowhere else.
// Operations: { type: "create", card, column, before }
//             { type: "update", id, fields }
//             { type: "move",   id, column, before }   before = card id or null (end)
//             { type: "delete", id }
// A move names the card it lands in front of, not an index: an index goes stale
// the moment another tab moves a card above it, a neighbour does not.
(function (root, factory) {
  if (typeof module === "object" && module.exports) module.exports = factory();
  else root.BoardOps = factory();
})(typeof self !== "undefined" ? self : this, function () {
  "use strict";

  const LIMITS = { title: 160, owner: 40, tag: 24, tags: 8, item: 120, items: 30 };
  const ID = /^[A-Za-z0-9_-]{4,40}$/;
  const DAY = /^\d{4}-\d{2}-\d{2}$/;

  const str = (v, n) => (typeof v === "string" ? v.trim().slice(0, n) : "");

  // Coerce whatever arrived into a valid card field set; unknown keys are dropped.
  function clean(fields) {
    const out = {};
    if ("title" in fields) out.title = str(fields.title, LIMITS.title) || "Untitled";
    if ("owner" in fields) out.owner = str(fields.owner, LIMITS.owner);
    if ("tags" in fields) {
      const tags = Array.isArray(fields.tags) ? fields.tags : [];
      out.tags = [...new Set(tags.map((t) => str(t, LIMITS.tag).toLowerCase()).filter(Boolean))].slice(0, LIMITS.tags);
    }
    if ("due" in fields) out.due = typeof fields.due === "string" && DAY.test(fields.due) ? fields.due : null;
    if ("checklist" in fields) {
      const items = Array.isArray(fields.checklist) ? fields.checklist : [];
      out.checklist = items
        .map((i) => ({ text: str(i && i.text, LIMITS.item), done: !!(i && i.done) }))
        .filter((i) => i.text)
        .slice(0, LIMITS.items);
    }
    return out;
  }

  function columnOf(board, id) {
    return board.columns.find((c) => c.cards.includes(id)) || null;
  }

  // Copy the parts an operation touches; the rest is shared, so React sees a
  // new object only where something changed.
  function shallow(board) {
    return { ...board, columns: board.columns.map((c) => ({ ...c, cards: c.cards.slice() })), cards: { ...board.cards } };
  }

  function insert(col, id, before) {
    const i = before ? col.cards.indexOf(before) : -1;
    if (i < 0) col.cards.push(id);
    else col.cards.splice(i, 0, id);
  }

  // The last column is "done": entering it stamps the card, leaving clears it.
  // The chart reads created/done, so throughput needs no separate log.
  function stampDone(board, card, colId, at) {
    const last = board.columns[board.columns.length - 1].id;
    const done = colId === last ? card.done || at : null;
    return done === card.done ? card : { ...card, done };
  }

  // Apply one operation; returns a new board, or throws Error with .status.
  function apply(board, op, at) {
    const fail = (status, msg) => Object.assign(new Error(msg), { status });
    if (!op || typeof op !== "object") throw fail(400, "operation must be an object");
    at = at || new Date().toISOString();
    const b = shallow(board);
    const col = (id) => b.columns.find((c) => c.id === id) || null;

    switch (op.type) {
      case "create": {
        const target = col(op.column) || b.columns[0];
        const id = typeof op.card?.id === "string" && ID.test(op.card.id) && !b.cards[op.card.id] ? op.card.id : newId();
        const card = { id, title: "Untitled", owner: "", tags: [], due: null, checklist: [], ...clean(op.card || {}), created: at, done: null };
        b.cards[id] = stampDone(b, card, target.id, at);
        insert(target, id, op.before);
        return b;
      }
      case "update": {
        const card = b.cards[op.id];
        if (!card) throw fail(404, "no card " + op.id);
        b.cards[op.id] = { ...card, ...clean(op.fields || {}) };
        return b;
      }
      case "move": {
        const card = b.cards[op.id];
        if (!card) throw fail(404, "no card " + op.id);
        const target = col(op.column);
        if (!target) throw fail(400, "no column " + op.column);
        if (op.before === op.id) return b;
        const from = b.columns.find((c) => c.cards.includes(op.id));
        from.cards.splice(from.cards.indexOf(op.id), 1);
        insert(target, op.id, op.before);
        b.cards[op.id] = stampDone(b, card, target.id, at);
        return b;
      }
      case "delete": {
        if (!b.cards[op.id]) throw fail(404, "no card " + op.id);
        const from = b.columns.find((c) => c.cards.includes(op.id));
        from.cards.splice(from.cards.indexOf(op.id), 1);
        delete b.cards[op.id];
        return b;
      }
      default:
        throw fail(400, "unknown operation " + op.type);
    }
  }

  // A whole board arriving by PUT: keep the columns' shape, re-clean every
  // card, and drop ids that are listed twice or point at nothing.
  function validate(input) {
    if (!input || !Array.isArray(input.columns) || !input.cards || typeof input.cards !== "object")
      throw Object.assign(new Error("board needs columns[] and cards{}"), { status: 400 });
    const seen = new Set(), cards = {};
    const columns = input.columns.slice(0, 12).map((c, i) => ({
      id: ID.test(c && c.id) ? c.id : "col" + i,
      title: str(c && c.title, 40) || "Column " + (i + 1),
      cards: (Array.isArray(c && c.cards) ? c.cards : []).filter((id) => {
        const card = input.cards[id];
        if (seen.has(id) || !card || !ID.test(id)) return false;
        seen.add(id);
        cards[id] = {
          id, title: "Untitled", owner: "", tags: [], due: null, checklist: [],
          ...clean(card),
          created: typeof card.created === "string" ? card.created : new Date().toISOString(),
          done: typeof card.done === "string" ? card.done : null,
        };
        return true;
      }),
    }));
    if (!columns.length) throw Object.assign(new Error("board needs at least one column"), { status: 400 });
    return { version: Number(input.version) || 0, updated: input.updated || null, columns, cards };
  }

  function newId() {
    const c = typeof crypto !== "undefined" ? crypto : null;
    if (c && c.randomUUID) return "c" + c.randomUUID().replace(/-/g, "").slice(0, 12);
    return "c" + Math.random().toString(36).slice(2, 14);
  }

  return { apply, validate, clean, columnOf, newId, LIMITS };
});
