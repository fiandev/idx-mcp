// Shared browser helpers: API access, number formatting, small renderers.
// Plain ES module, no build step. Served with script-src 'self'.

export const API = "/api";
// The API gives up on a slow source well before this; past it the terminal
// reports the request instead of leaving it pending.
const API_TIMEOUT_MS = 60_000;

export async function api(path) {
  let r;
  try {
    r = await fetch(`${API}/${path}`, { headers: { Accept: "application/json" }, signal: AbortSignal.timeout?.(API_TIMEOUT_MS) });
  } catch (err) {
    const e = new Error(err?.name === "TimeoutError" ? `no answer within ${API_TIMEOUT_MS / 1000}s; try again` : `network error: ${err?.message || err}`);
    e.code = "network";
    e.exit = 3;
    throw e;
  }
  const body = await r.json().catch(() => ({ error: { code: "provider", message: `HTTP ${r.status}` }, exit_code: 3 }));
  if (!r.ok || body.error) {
    const e = new Error(body.error?.message || `HTTP ${r.status}`);
    e.code = body.error?.code || "provider";
    e.exit = body.exit_code ?? 3;
    throw e;
  }
  return body;
}

const DASH = "—";
const nf0 = new Intl.NumberFormat("en-US", { maximumFractionDigits: 0 });
const nf2 = new Intl.NumberFormat("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 });

export const fmt = {
  price(v) { if (v == null) return DASH; return Number.isInteger(v) || Math.abs(v - Math.round(v)) < 1e-9 ? nf0.format(v) : nf2.format(v); },
  int(v) { return v == null ? DASH : nf0.format(v); },
  num(v, d = 2) { return v == null ? DASH : new Intl.NumberFormat("en-US", { minimumFractionDigits: d, maximumFractionDigits: d }).format(v); },
  ratio(v) { return v == null ? DASH : nf2.format(v); },
  pct(v) { if (v == null) return DASH; const s = nf2.format(v); return v > 0 ? `+${s}%` : `${s}%`; },
  pctPlain(v) { return v == null ? DASH : `${nf2.format(v)}%`; },
  change(v) { if (v == null) return DASH; const s = fmt.price(Math.abs(v)); return v > 0 ? `+${s}` : v < 0 ? `-${s}` : s; },
  compact(v) {
    if (v == null) return DASH;
    const a = Math.abs(v);
    const [n, s] = a >= 1e12 ? [v / 1e12, "T"] : a >= 1e9 ? [v / 1e9, "B"] : a >= 1e6 ? [v / 1e6, "M"] : a >= 1e3 ? [v / 1e3, "K"] : [v, ""];
    return `${new Intl.NumberFormat("en-US", { maximumFractionDigits: Math.abs(n) >= 100 ? 1 : 2 }).format(n)}${s}`;
  },
  wib(iso) { if (!iso) return DASH; const m = String(iso).match(/(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})/); return m ? `${m[1]}-${m[2]}-${m[3]} ${m[4]}:${m[5]} WIB` : iso; },
  time(iso) { if (!iso) return DASH; const m = String(iso).match(/T(\d{2}):(\d{2})/); return m ? `${m[1]}:${m[2]} WIB` : iso; },
  date(iso) { return iso ? String(iso).slice(0, 10) : DASH; },
  text(v) { return v == null || v === "" ? DASH : String(v); },
};

export const signClass = (v) => (v > 0 ? "up" : v < 0 ? "dn" : "");

/** Builds a table. cols: [{h, k|f, cls, l}] ; rows: objects. */
export function table(cols, rows, opts = {}) {
  const t = document.createElement("table");
  t.className = "tbl";
  const thead = t.createTHead();
  const hr = thead.insertRow();
  for (const c of cols) {
    const th = document.createElement("th");
    th.textContent = c.h;
    if (c.l) th.classList.add("l");
    th.scope = "col";
    hr.appendChild(th);
  }
  const tb = t.createTBody();
  for (const row of rows) {
    const tr = tb.insertRow();
    if (opts.key) tr.dataset.key = row[opts.key];
    for (const c of cols) {
      const td = tr.insertCell();
      const val = c.f ? c.f(row) : row[c.k];
      if (val instanceof Node) td.appendChild(val);
      else td.textContent = val == null ? DASH : val;
      if (c.l) td.classList.add("l");
      if (c.cls) { const cl = typeof c.cls === "function" ? c.cls(row) : c.cls; if (cl) td.className += ` ${cl}`; }
      if (c.name) td.dataset.col = c.name;
      if (c.title) { const t = c.title(row); if (t) td.title = t; }
    }
  }
  return t;
}

/** Horizontal scroll container with a visible cue on touch screens. */
export function wrapScroll(node) {
  const d = document.createElement("div");
  d.className = "tbl-wrap";
  d.appendChild(node);
  const frag = document.createDocumentFragment();
  const cue = el("p", { class: "scroll-cue", text: "scroll \u2192 for more columns" });
  frag.append(d, cue);
  const check = () => {
    const more = d.scrollWidth - d.clientWidth - d.scrollLeft > 4;
    d.classList.toggle("scrollable", more);
    cue.classList.toggle("on", d.scrollWidth > d.clientWidth + 4);
  };
  requestAnimationFrame(check);
  d.addEventListener("scroll", check, { passive: true });
  if (typeof ResizeObserver !== "undefined") new ResizeObserver(check).observe(d);
  return frag;
}

/** Escapes a value for the few renderers that build markup instead of nodes. */
export function esc(v) {
  return String(v ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
}

/**
 * Tagged template for `el(..., { html })`: the literal markup in the template
 * stays as written, every interpolated value is escaped. Use it for the one or
 * two spans where `el("b", …)` would be noisier; anything longer is easier to
 * read as nodes.
 */
export function h(strings, ...values) {
  return strings.reduce((out, s, i) => out + s + (i < values.length ? esc(values[i]) : ""), "");
}

export function el(tag, attrs = {}, children = []) {
  const n = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "class") n.className = v;
    else if (k === "text") n.textContent = v;
    // Only `h\`…\`` or another already-escaped string, never raw provider data.
    else if (k === "html") n.innerHTML = v;
    else if (k.startsWith("on")) n.addEventListener(k.slice(2), v);
    else n.setAttribute(k, v);
  }
  for (const c of [].concat(children)) if (c != null) n.appendChild(typeof c === "string" ? document.createTextNode(c) : c);
  return n;
}

/** Sparkline from [[ts, price], ...]. */
export function sparkline(points, { width = 600, height = 44, cls = "tape" } = {}) {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", `0 0 ${width} ${height}`);
  svg.setAttribute("preserveAspectRatio", "none");
  svg.setAttribute("class", cls);
  svg.setAttribute("role", "img");
  if (points.length < 2) return svg;
  const ys = points.map((p) => p[1]);
  const min = Math.min(...ys), max = Math.max(...ys), span = max - min || 1;
  const x = (i) => (i / (points.length - 1)) * width;
  const y = (v) => height - 2 - ((v - min) / span) * (height - 4);
  const d = points.map((p, i) => `${i ? "L" : "M"}${x(i).toFixed(1)},${y(p[1]).toFixed(1)}`).join("");
  const up = ys[ys.length - 1] >= ys[0];
  const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
  path.setAttribute("d", d);
  path.setAttribute("fill", "none");
  path.setAttribute("stroke", up ? "#0f6b31" : "#b3122f");
  path.setAttribute("stroke-width", "1.6");
  path.setAttribute("vector-effect", "non-scaling-stroke");
  svg.appendChild(path);
  const base = document.createElementNS("http://www.w3.org/2000/svg", "line");
  base.setAttribute("x1", 0); base.setAttribute("x2", width);
  base.setAttribute("y1", y(ys[0])); base.setAttribute("y2", y(ys[0]));
  base.setAttribute("stroke", "#b9bcb5"); base.setAttribute("stroke-dasharray", "3 4"); base.setAttribute("vector-effect", "non-scaling-stroke");
  svg.insertBefore(base, path);
  svg.setAttribute("aria-label", `index tape from ${fmt.price(ys[0])} to ${fmt.price(ys[ys.length - 1])}`);
  return svg;
}

/** Marks cells whose text changed since the last render (the re-ink moment). */
export function reink(oldTable, newTable) {
  if (!oldTable) return;
  const prev = new Map();
  for (const tr of oldTable.tBodies[0]?.rows || []) prev.set(tr.dataset.key, [...tr.cells].map((c) => c.textContent));
  for (const tr of newTable.tBodies[0]?.rows || []) {
    const was = prev.get(tr.dataset.key);
    if (!was) continue;
    [...tr.cells].forEach((c, i) => { if (was[i] !== c.textContent) c.classList.add("reink"); });
  }
}

export function copyButton(text) {
  const b = el("button", { class: "copy", type: "button", text: "Copy" });
  b.addEventListener("click", async () => {
    try { await navigator.clipboard.writeText(text); b.textContent = "Copied"; setTimeout(() => (b.textContent = "Copy"), 1400); }
    catch { b.textContent = "Select & copy"; }
  });
  return b;
}
