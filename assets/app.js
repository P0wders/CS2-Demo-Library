/* cs2_demo wiki — renders wiki/*.md with a docs layout. No build step. */

const REPO = "https://github.com/P0wders/CS2-Demo-Library";
const BRANCH = "docs/wiki";

// ───────── Navigation: [file name in wiki/, label] ─────────
const NAV = [
  { title: "Getting started", items: [
    ["Home", "Introduction"], ["Installation", "Installation"], ["Getting-started", "Getting started"] ] },
  { title: "Guides", items: [
    ["Match-statistics", "Match statistics"], ["Game-events", "Game events"], ["Replay-viewer", "Replay viewer"],
    ["Visitor", "Writing a Visitor"], ["Game-state", "Reading game state"], ["Parse-options", "Choosing what to decode"],
    ["Many-demos", "Many demos at once"], ["JSON-export", "Export to JSON"] ] },
  { title: "Reference", items: [
    ["Stats-reference", "MatchStats reference"], ["Definitions", "Definitions"], ["Aim-metrics", "Aim metrics"], ["Scouting", "Scouting"] ] },
  { title: "Examples", items: [
    ["Examples", "All examples"], ["Dev-tools", "Parser dev tools"] ] },
  { title: "Internals", items: [
    ["How-it-works", "How it works"], ["Performance", "Performance"], ["Compatibility", "Compatibility"], ["Development", "Development"] ] },
];
const FLAT = NAV.flatMap(g => g.items.map(([id, label]) => ({ id, label, group: g.title })));
const byId = id => FLAT.find(f => f.id.toLowerCase() === String(id).toLowerCase());

// API names for search, each pointing at the page that explains it.
const API = [
  ["Demo::open", "Getting-started"], ["Demo::read_header", "Getting-started"], ["Demo::match_stats", "Match-statistics"],
  ["Demo::for_each_event", "Game-events"], ["Demo::parse", "Visitor"], ["Visitor", "Visitor"], ["Context", "Game-state"],
  ["StatsCollector", "Visitor"], ["ParseOptions::events_only", "Parse-options"], ["parse_single_threaded", "Parse-options"],
  ["match_stats_many", "Many-demos"], ["Scouting::team", "Scouting"], ["Scouting::alias", "Scouting"],
  ["PositionRecorder", "Replay-viewer"], ["MapOverview", "Replay-viewer"], ["to_radar", "Replay-viewer"],
  ["round_kills", "Stats-reference"], ["lineups", "Stats-reference"], ["entity_diagnostics", "Compatibility"],
  ["KAST", "Definitions"], ["ADR", "Definitions"], ["HLTV Rating 1.0", "Definitions"], ["BuyThresholds", "Definitions"],
  ["First-shot accuracy", "Aim-metrics"], ["Crosshair placement", "Aim-metrics"], ["dump_schema", "Dev-tools"],
  ["serde", "JSON-export"], ["user-messages", "Installation"],
];

// ───────── Icons ─────────
const svg = (d, extra = "") => `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" ${extra}>${d}</svg>`;
const I = {
  search: svg('<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>'),
  github: '<svg viewBox="0 0 24 24" fill="currentColor"><path d="M12 .5a11.5 11.5 0 0 0-3.64 22.41c.58.1.79-.25.79-.56v-2c-3.2.7-3.88-1.37-3.88-1.37-.53-1.33-1.28-1.69-1.28-1.69-1.05-.71.08-.7.08-.7 1.16.08 1.77 1.19 1.77 1.19 1.03 1.77 2.7 1.26 3.36.96.1-.75.4-1.26.73-1.55-2.55-.29-5.24-1.28-5.24-5.69 0-1.26.45-2.29 1.19-3.1-.12-.29-.52-1.46.11-3.05 0 0 .97-.31 3.17 1.18a11 11 0 0 1 5.77 0c2.2-1.49 3.17-1.18 3.17-1.18.63 1.59.23 2.76.11 3.05.74.81 1.19 1.84 1.19 3.1 0 4.42-2.7 5.4-5.26 5.68.41.36.78 1.06.78 2.14v3.17c0 .31.21.67.8.56A11.5 11.5 0 0 0 12 .5Z"/></svg>',
  sun: svg('<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>'),
  moon: svg('<path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8Z"/>'),
  menu: svg('<path d="M4 7h16M4 12h16M4 17h10"/>'),
  copy: svg('<rect x="9" y="9" width="12" height="12" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h10"/>'),
  info: svg('<circle cx="12" cy="12" r="9"/><path d="M12 11v5M12 8h.01"/>'),
  tip: svg('<path d="M9 18h6M10 21h4M12 3a6 6 0 0 0-3.5 10.9c.6.5 1 1.2 1 2V16h5v-.1c0-.8.4-1.5 1-2A6 6 0 0 0 12 3Z"/>'),
  warn: svg('<path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0Z"/><path d="M12 9v4M12 17h.01"/>'),
  arrow: svg('<path d="M5 12h14M13 6l6 6-6 6"/>'),
  page: svg('<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/>'),
  hash: svg('<path d="M5 9h14M5 15h14M10 3 8 21M16 3l-2 18"/>'),
  code: svg('<path d="m8 8-4 4 4 4M16 8l4 4-4 4"/>'),
  board: svg('<rect x="3" y="4" width="18" height="16" rx="2.5"/><path d="M3 9h18M9 9v11"/>', 'stroke-width="1.8"'),
  coins: svg('<ellipse cx="9" cy="7" rx="6" ry="3"/><path d="M3 7v5c0 1.7 2.7 3 6 3s6-1.3 6-3V7"/><path d="M15 11.5c3.3 0 6 1.3 6 3v3c0 1.7-2.7 3-6 3-2 0-3.8-.5-4.9-1.3"/>', 'stroke-width="1.8"'),
  arc: svg('<path d="M4 19C6 9 14 5 20 7"/><circle cx="4" cy="19" r="1.6"/><circle cx="20" cy="7" r="2.6"/>', 'stroke-width="1.8"'),
  target: svg('<circle cx="12" cy="12" r="8"/><circle cx="12" cy="12" r="3"/><path d="M12 2v4M12 18v4M2 12h4M18 12h4"/>', 'stroke-width="1.8"'),
  layers: svg('<path d="m12 3 9 5-9 5-9-5z"/><path d="m3 13 9 5 9-5"/>', 'stroke-width="1.8"'),
  radar: svg('<circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="5"/><path d="M12 12 18 6"/><circle cx="15.5" cy="15" r="1" fill="currentColor"/>', 'stroke-width="1.8"'),
};
const LOGO = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><circle cx="12" cy="12" r="7.5"/><path d="M12 1.5v5M12 17.5v5M1.5 12h5M17.5 12h5"/></svg>';

// ───────── Helpers ─────────
const $ = s => document.querySelector(s);
const esc = s => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
const slug = s => s.toLowerCase().replace(/<[^>]+>/g, "").replace(/&[a-z]+;/g, "").replace(/[`*]/g, "")
  .replace(/[^\p{L}\p{N}\s-]/gu, "").trim().replace(/\s+/g, "-");
const route = (id, anchor) => `#/${id}${anchor ? "/" + anchor : ""}`;

// ───────── Syntax highlighting (Rust, TOML, shell) ─────────
function highlight(src, lang) {
  if (!["rust", "rs", "toml", "sh", "bash", "shell", "powershell"].includes(lang)) return esc(src);
  const rules = lang === "toml" ? [
    [/#.*$/m, "c"], [/"(?:[^"\\]|\\.)*"/, "s"], [/^\s*\[[^\]]+\]/m, "t"], [/\b\d+\b/, "n"], [/\b[a-z_][\w.-]*(?=\s*=)/, "k"],
  ] : lang === "rust" || lang === "rs" ? [
    [/\/\/.*$/m, "c"], [/"(?:[^"\\]|\\.)*"/, "s"],
    [/\b(use|fn|let|mut|for|in|if|else|match|return|struct|impl|pub|self|Self|Some|None|Ok|Err|as|mod|where|move|ref|true|false|while|loop|const|enum|trait|type|dyn)\b/, "k"],
    [/\b[a-z_]+!/, "m"], [/#\[[^\]]*\]/, "c"], [/\b[A-Z][A-Za-z0-9]*\b/, "t"], [/\b\d+(?:\.\d+)?\b/, "n"],
  ] : [
    [/#.*$/m, "c"], [/"(?:[^"\\]|\\.)*"/, "s"], [/^\s*(cargo|\$env)\b/m, "k"], [/\s--?[\w-]+/, "t"], [/\b[A-Z][A-Z0-9_]{2,}\b/, "m"],
  ];
  let out = "", rest = src;
  while (rest.length) {
    let best = null;
    for (const [re, cls] of rules) {
      const m = re.exec(rest);
      if (m && m[0].length && (best === null || m.index < best.i)) best = { i: m.index, len: m[0].length, cls, txt: m[0] };
    }
    if (!best) { out += esc(rest); break; }
    out += esc(rest.slice(0, best.i)) + `<span class="tk-${best.cls}">${esc(best.txt)}</span>`;
    rest = rest.slice(best.i + best.len);
  }
  return out;
}

// ───────── Markdown ─────────
function linkHref(url) {
  const m = /^([\w-]+)\.md(?:#(.*))?$/.exec(url);
  if (m && byId(m[1])) return { href: route(byId(m[1]).id, m[2]), ext: false };
  if (url.startsWith("#")) return { href: url, ext: false };
  return { href: url, ext: /^https?:/.test(url) };
}
function inline(text) {
  const codes = [];
  let s = text.replace(/`([^`]+)`/g, (_, c) => `\u0000${codes.push(`<code>${esc(c)}</code>`) - 1}\u0000`);
  s = esc(s);
  s = s.replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, (_, t, u) => {
    const { href, ext } = linkHref(u.replace(/&amp;/g, "&"));
    return `<a class="link" href="${esc(href)}"${ext ? ' target="_blank" rel="noopener"' : ""}>${t}</a>`;
  });
  s = s.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>").replace(/(^|[\s(])\*([^*\s][^*]*)\*/g, "$1<em>$2</em>");
  return s.replace(/\u0000(\d+)\u0000/g, (_, i) => codes[i]);
}
const splitRow = l => l.trim().replace(/^\|/, "").replace(/\|$/, "").split(/(?<!\\)\|/).map(c => c.trim().replace(/\\\|/g, "|"));

function codeBlock(src, lang, title) {
  return `<div class="code"><div class="code-head"><span>${esc(title || lang || "text")}</span>
    <button class="copy" data-copy>${I.copy}<span>Copy</span></button></div>
    <pre><code>${highlight(src.replace(/\n$/, ""), lang)}</code></pre></div>`;
}
function table(head, align, rows) {
  const cls = i => align[i] === "right" ? ' class="num"' : align[i] === "center" ? ' style="text-align:center"' : "";
  return `<div class="table-wrap"><table><thead><tr>${head.map((h, i) => `<th${cls(i)}>${inline(h)}</th>`).join("")}</tr></thead>
    <tbody>${rows.map(r => `<tr>${head.map((_, i) => `<td${cls(i)}>${inline(r[i] || "")}</td>`).join("")}</tr>`).join("")}</tbody></table></div>`;
}
function scoreboard(title, tables) {
  const m = /\*\*(.+?)\s+(\d+)\s*[–-]\s*(\d+)\s+(.+?)\*\*\s*(?:\((.+)\))?/.exec(title || "");
  const [a, sa, sb, b, map] = m ? m.slice(1) : [tables[0]?.head[0], "", "", tables[1]?.head[0], ""];
  const head = tables[0].head;
  const ri = head.findIndex(h => /rating/i.test(h)), di = head.findIndex(h => /^\+\/[−-]$/.test(h));
  const rows = t => t.rows.map(r => `<tr>${r.map((c, i) => {
    if (i === 0) return `<td>${inline(c)}</td>`;
    if (i === ri) { const v = parseFloat(c); return `<td class="num"><span class="rating"><span class="bar"><i class="${v >= 1.2 ? "hi" : ""}" style="width:${Math.min(v / 1.8, 1) * 100}%"></i></span>${esc(c)}</span></td>`; }
    if (i === di) return `<td class="num ${c.startsWith("+") ? "pos" : c.startsWith("-") ? "neg" : ""}">${esc(c)}</td>`;
    return `<td class="num">${esc(c)}</td>`;
  }).join("")}</tr>`).join("");
  const win = +sa >= +sb;
  return `<div class="scoreboard"><div class="sb-head">
    <div class="sb-team">${esc(a)}</div>
    <div class="sb-score"><span class="${win ? "" : "lose"}">${sa}</span><span class="sep">–</span><span class="${win ? "lose" : ""}">${sb}</span></div>
    <div class="sb-team right">${esc(b)}</div>${map ? `<div class="sb-map">${esc(map)}</div>` : ""}</div>
    <div style="overflow-x:auto"><table><thead><tr><th>Player</th>${head.slice(1).map(h => `<th class="num">${esc(h)}</th>`).join("")}</tr></thead>
    <tbody>${tables.map(t => `<tr class="group"><td colspan="${head.length}">${esc(t.head[0])}</td></tr>${rows(t)}`).join("")}</tbody></table></div></div>`;
}

function markdown(md) {
  const lines = md.replace(/\r/g, "").split("\n");
  const out = [], toc = [];
  let title = "", lead = "", i = 0, afterH1 = false;
  const isTableAt = j => /^\s*\|/.test(lines[j] || "") && /^\s*\|?\s*:?-{2,}/.test(lines[j + 1] || "");
  const readTable = () => {
    const head = splitRow(lines[i]);
    const align = splitRow(lines[i + 1]).map(c => c.endsWith(":") ? (c.startsWith(":") ? "center" : "right") : "left");
    const rows = []; i += 2;
    while (i < lines.length && /^\s*\|/.test(lines[i])) rows.push(splitRow(lines[i++]));
    return { head, align, rows };
  };
  while (i < lines.length) {
    const line = lines[i];
    if (!line.trim()) { i++; continue; }
    let m;
    if ((m = /^```(\w*)\s*(?:title="([^"]*)")?/.exec(line))) {
      const buf = []; i++;
      while (i < lines.length && !/^```\s*$/.test(lines[i])) buf.push(lines[i++]);
      i++; out.push(codeBlock(buf.join("\n"), m[1], m[2])); afterH1 = false; continue;
    }
    if ((m = /^<!--\s*scoreboard\b.*-->/.exec(line))) {
      i++; let t = "";
      while (i < lines.length && !lines[i].trim()) i++;
      if (/^\*\*/.test(lines[i])) t = lines[i++];
      const tables = [];
      for (;;) { while (i < lines.length && !lines[i].trim()) i++; if (isTableAt(i)) tables.push(readTable()); else break; }
      if (tables.length) out.push(scoreboard(t, tables));
      continue;
    }
    if (/^<!--/.test(line)) { while (i < lines.length && !/-->/.test(lines[i])) i++; i++; continue; }
    if ((m = /^(#{1,4})\s+(.*)$/.exec(line))) {
      const lvl = m[1].length, txt = m[2].trim(); i++;
      if (lvl === 1 && !title) { title = inline(txt); afterH1 = true; continue; }
      const id = slug(txt);
      if (lvl <= 3) toc.push([id, txt.replace(/`/g, ""), lvl === 3]);
      out.push(`<h${lvl} id="${id}">${inline(txt)}<a class="anchor" href="#${id}" aria-label="Link to section">#</a></h${lvl}>`);
      afterH1 = false; continue;
    }
    if (/^>/.test(line)) {
      const buf = [];
      while (i < lines.length && /^>/.test(lines[i])) buf.push(lines[i++].replace(/^>\s?/, ""));
      const kind = /^\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\]/i.exec(buf[0]);
      if (kind) buf.shift();
      const k = kind ? kind[1].toLowerCase() : "note";
      const icon = k === "tip" ? I.tip : k === "warning" || k === "caution" ? I.warn : I.info;
      const body = buf.join("\n").split(/\n\s*\n/).map(p => `<p>${inline(p.replace(/\n/g, " "))}</p>`).join("");
      out.push(kind ? `<div class="callout ${k}">${icon}<div class="callout-body">${body}</div></div>` : `<blockquote>${body}</blockquote>`);
      afterH1 = false; continue;
    }
    if (isTableAt(i)) { const t = readTable(); out.push(table(t.head, t.align, t.rows)); afterH1 = false; continue; }
    if (/^\s*([-*]|\d+\.)\s+/.test(line)) {
      const ordered = /^\s*\d+\./.test(line), items = [];
      while (i < lines.length && (/^\s*([-*]|\d+\.)\s+/.test(lines[i]) || (/^\s{2,}\S/.test(lines[i]) && items.length))) {
        if (/^\s*([-*]|\d+\.)\s+/.test(lines[i])) items.push(lines[i].replace(/^\s*([-*]|\d+\.)\s+/, ""));
        else items[items.length - 1] += " " + lines[i].trim();
        i++;
      }
      out.push(`<${ordered ? "ol" : "ul"}>${items.map(t => `<li>${inline(t)}</li>`).join("")}</${ordered ? "ol" : "ul"}>`);
      afterH1 = false; continue;
    }
    if (/^(-{3,}|\*{3,})\s*$/.test(line)) { out.push("<hr>"); i++; continue; }
    const buf = [];
    while (i < lines.length && lines[i].trim() && !/^(```|#{1,4}\s|>|\s*\||\s*[-*]\s|<!--)/.test(lines[i])) buf.push(lines[i++]);
    if (!buf.length) { buf.push(lines[i++]); }
    const html = inline(buf.join(" "));
    if (afterH1 && !lead) { lead = html; afterH1 = false; continue; }
    out.push(`<p>${html}</p>`);
  }
  return { title, lead, html: out.join("\n"), toc };
}

// ───────── Loading pages ─────────
const cache = new Map();
function load(id) {
  if (!cache.has(id)) cache.set(id, fetch(`wiki/${id}.md`).then(r => {
    if (!r.ok) throw new Error(r.status);
    return r.text();
  }).then(md => ({ md, ...markdown(md) })).catch(e => { cache.delete(id); throw e; }));
  return cache.get(id);
}

// ───────── Shell ─────────
function renderShell() {
  document.body.innerHTML = `
  <header class="topbar">
    <button class="icon-btn menu-btn" id="menu" aria-label="Open menu">${I.menu}</button>
    <a class="brand" href="#/Home"><span class="brand-mark">${LOGO}</span><span class="brand-name">cs2_demo</span><span class="brand-tag">wiki</span></a>
    <button class="search-btn" id="search">${I.search}<span>Search the wiki…</span><kbd>Ctrl K</kbd></button>
    <nav class="top-links">
      <a href="#/Examples"><span>Examples</span></a>
      <a class="gh" href="${REPO}" target="_blank" rel="noopener">${I.github}<span>GitHub</span></a>
      <button class="icon-btn" id="theme" aria-label="Toggle theme"></button>
    </nav>
  </header>
  <div class="shell">
    <aside class="sidebar" id="sidebar">
      ${NAV.map(g => `<div class="nav-group"><div class="nav-title">${g.title}</div>
        ${g.items.map(([id, l]) => `<a href="${route(id)}" data-id="${id}">${l}</a>`).join("")}</div>`).join("")}
      <div class="side-foot">Apache-2.0 · <a href="${REPO}" target="_blank" rel="noopener">P0wders/CS2-Demo-Library</a><br>Tested on CS2 builds 10680–10955</div>
    </aside>
    <div class="scrim" id="scrim"></div>
    <main class="main" id="main"></main>
  </div>
  <div class="overlay" id="overlay"><div class="palette" role="dialog" aria-label="Search">
    <div class="palette-input">${I.search}<input id="q" placeholder="Search pages, sections, APIs…" autocomplete="off" spellcheck="false"></div>
    <div class="palette-results" id="results"></div>
    <div class="palette-foot"><span><kbd>↑</kbd> <kbd>↓</kbd> navigate</span><span><kbd>↵</kbd> open</span><span><kbd>esc</kbd> close</span></div>
  </div></div>`;
}

// ───────── Home ─────────
function homeHtml(page) {
  return `
  <section class="hero">
    <div class="eyebrow"><b>Rust</b> Parser &amp; match analyzer for Counter-Strike 2</div>
    <h1>Every tick of a CS2 demo, <em>in two seconds.</em></h1>
    <p class="lead">cs2_demo reads <code>.dem</code> files from scratch and gives you the game events, the full game state at every tick, and the numbers a team analyst works with.</p>
    <div class="actions">
      <a class="btn primary" href="#/Getting-started">Get started ${I.arrow}</a>
      <a class="btn" href="#/Installation">Installation</a>
      <a class="btn" href="${REPO}" target="_blank" rel="noopener">${I.github} GitHub</a>
    </div>
    ${codeBlock(`let stats = cs2_demo::Demo::open("match.dem")?.match_stats()?;
for p in &stats.players {
    println!("{:16} {}-{}-{}  ADR {:.1}  rating {:.2}",
        p.name, p.kills, p.deaths, p.assists, p.adr(), p.rating());
}`, "rust", "main.rs")}
  </section>

  <div class="stats">
    <div class="stat"><b>0.13<small>s</small></b><span>Game events, 342 MB demo</span></div>
    <div class="stat"><b>2.1<small>s</small></b><span>Full parse + every statistic</span></div>
    <div class="stat"><b>12<small>MB</small></b><span>Memory beside the mapped file</span></div>
    <div class="stat"><b>32</b><span>Demos tested, bit-exact</span></div>
  </div>

  <div class="section-label"><h2 id="start">Where to start</h2></div>
  <div class="home-md">${page.lead ? `<p>${page.lead}</p>` : ""}${page.html}</div>

  <div class="section-label"><h2 id="features">What you get</h2><a href="#/Stats-reference">MatchStats reference →</a></div>
  <div class="cards">
    <a class="card" href="#/Stats-reference"><div class="card-icon">${I.board}</div><h3>Scoreboard</h3><p>K/D/A, ADR, KAST, HLTV Rating 1.0, opening duels, trades and multi-kills, split by side.</p></a>
    <a class="card" href="#/Definitions/economy"><div class="card-icon">${I.coins}</div><h3>Rounds &amp; economy</h3><p>Every team's buy, bomb sites, clutches and where everyone stood.</p></a>
    <a class="card" href="#/Definitions/utility"><div class="card-icon">${I.arc}</div><h3>Utility</h3><p>Every grenade with its lineup, flight and effect: blinds, damage, smoke and fire duration.</p></a>
    <a class="card" href="#/Aim-metrics"><div class="card-icon">${I.target}</div><h3>Aim</h3><p>Accuracy, first-shot accuracy, counter-strafing, crosshair placement, time to kill.</p></a>
    <a class="card" href="#/Scouting"><div class="card-icon">${I.layers}</div><h3>Scouting</h3><p>Map records, side and pistol win rates, setups and lineups across many demos.</p></a>
    <a class="card" href="#/Replay-viewer"><div class="card-icon">${I.radar}</div><h3>2D replay</h3><p>Players, grenades, smokes, fires and the bomb over time, on the map's radar.</p></a>
  </div>

  <div class="split">
    <div class="mini-list"><h4>Guides <span>8</span></h4>
      <a href="#/Visitor"><code>Visitor</code><span>Your own analysis</span></a>
      <a href="#/Game-state"><code>Context</code><span>Players, teams, raw properties</span></a>
      <a href="#/Parse-options"><code>ParseOptions</code><span>Decode only what you need</span></a>
      <a href="#/Many-demos"><code>match_stats_many</code><span>One demo per core</span></a>
    </div>
    <div class="mini-list"><h4>Examples <span>13</span></h4>
      <a href="#/Examples"><code>quick_start</code><span>Header, score, scoreboard</span></a>
      <a href="#/Examples"><code>kill_feed</code><span>Weapons, callouts, flags</span></a>
      <a href="#/Examples"><code>grenades</code><span>Effects and repeated lineups</span></a>
      <a href="#/Examples"><code>scouting</code><span>A team across many demos</span></a>
    </div>
  </div>`;
}

// ───────── Page render ─────────
let current = null;
function parseHash() {
  const [id, anchor] = location.hash.replace(/^#\/?/, "").split("/");
  return { id: id ? decodeURIComponent(id) : "Home", anchor: anchor ? decodeURIComponent(anchor) : null };
}
async function renderPage() {
  const { id, anchor } = parseHash();
  const item = byId(id);
  document.body.classList.remove("nav-open");
  if (item && current === item.id) { scrollToAnchor(anchor); return; }
  const main = $("#main");
  document.querySelectorAll(".nav-group a").forEach(a => a.classList.toggle("active", item && a.dataset.id === item.id));
  if (!item) {
    current = null; main.className = "main";
    main.innerHTML = `<article class="content notfound"><h1>Page not found</h1><p class="lead">There is no page called “${esc(id)}”.</p><a class="btn" href="#/Home">Back to the introduction</a></article>`;
    document.title = "Not found — cs2_demo wiki"; return;
  }
  current = item.id;
  main.className = "main" + (item.id === "Home" ? " wide" : "");
  main.innerHTML = `<article class="content"><div class="loading title"></div><div class="loading"></div><div class="loading" style="width:80%"></div></article>`;
  let page;
  try { page = await load(item.id); }
  catch {
    if (current !== item.id) return;
    main.innerHTML = `<article class="content notfound"><h1>${esc(item.label)}</h1><p class="lead">This page couldn't be loaded. Check your connection and try again.</p></article>`;
    return;
  }
  if (current !== item.id) return; // navigated away while loading
  document.title = item.id === "Home" ? "cs2_demo wiki" : `${item.label} — cs2_demo wiki`;
  if (item.id === "Home") {
    main.innerHTML = `<article class="content">${homeHtml(page)}</article>`;
  } else {
    const idx = FLAT.indexOf(item), prev = FLAT[idx - 1], next = FLAT[idx + 1];
    const toc = page.toc.length > 1 ? `<aside class="toc"><div class="toc-title">On this page</div>
      ${page.toc.map(([h, l, sub]) => `<a class="${sub ? "sub" : ""}" href="#${h}" data-h="${h}">${esc(l)}</a>`).join("")}</aside>` : "";
    main.innerHTML = `<article class="content">
      <div class="crumbs"><span>${item.group}</span><span>/</span><span>${item.label}</span></div>
      <h1>${page.title || esc(item.label)}</h1>
      ${page.lead ? `<p class="lead">${page.lead}</p>` : ""}
      ${page.html}
      <nav class="pager">
        ${prev ? `<a href="${route(prev.id)}"><small>Previous</small><span>${prev.label}</span></a>` : "<span></span>"}
        ${next ? `<a class="next" href="${route(next.id)}"><small>Next</small><span>${next.label}</span></a>` : ""}
      </nav>
      <div class="page-meta"><a href="${REPO}/edit/${BRANCH}/wiki/${item.id}.md" target="_blank" rel="noopener">Edit this page on GitHub</a>
        <a href="${REPO}/blob/${BRANCH}/wiki/${item.id}.md" target="_blank" rel="noopener">View source</a></div>
    </article>${toc}`;
    if (!toc) main.classList.add("wide");
  }
  bindPage();
  if (anchor) requestAnimationFrame(() => scrollToAnchor(anchor)); else window.scrollTo(0, 0);
}

function scrollToAnchor(anchor) {
  const el = anchor && document.getElementById(anchor);
  if (el) el.scrollIntoView({ behavior: "smooth" }); else if (!anchor) window.scrollTo({ top: 0 });
}

function bindPage() {
  document.querySelectorAll("[data-copy]").forEach(b => b.onclick = async () => {
    const text = b.closest(".code").querySelector("pre").innerText;
    try { await navigator.clipboard.writeText(text); } catch {
      const ta = Object.assign(document.createElement("textarea"), { value: text }); document.body.append(ta); ta.select(); document.execCommand("copy"); ta.remove();
    }
    const label = b.querySelector("span"); label.textContent = "Copied"; setTimeout(() => label.textContent = "Copy", 1400);
  });
  // Same-page anchors (#id) become #/Page/id so links stay shareable.
  document.querySelectorAll('#main a[href^="#"]:not([href^="#/"])').forEach(a => {
    a.setAttribute("href", route(current, a.getAttribute("href").slice(1)));
  });
  const links = [...document.querySelectorAll(".toc a")];
  if (links.length) {
    const visible = new Set();
    const obs = new IntersectionObserver(es => {
      es.forEach(e => e.isIntersecting ? visible.add(e.target.id) : visible.delete(e.target.id));
      const first = links.find(l => visible.has(l.dataset.h));
      if (first) links.forEach(l => l.classList.toggle("active", l === first));
    }, { rootMargin: "-70px 0px -65% 0px" });
    links.forEach(l => { const t = document.getElementById(l.dataset.h); t && obs.observe(t); });
    links[0].classList.add("active");
  }
}

// ───────── Theme ─────────
function setTheme(t) {
  document.documentElement.dataset.theme = t;
  $("#theme").innerHTML = t === "light" ? I.moon : I.sun;
  $("#theme").setAttribute("aria-label", t === "light" ? "Switch to dark theme" : "Switch to light theme");
  try { localStorage.setItem("theme", t); } catch {}
}

// ───────── Search ─────────
let index = null, sel = 0, shown = [];
async function buildIndex() {
  if (index) return index;
  const pages = await Promise.all(FLAT.map(f => load(f.id).then(p => ({ f, p })).catch(() => null)));
  index = [];
  for (const x of pages.filter(Boolean)) {
    const text = x.p.md.replace(/```[\s\S]*?```/g, " ").replace(/[#>*`|[\]()-]/g, " ").replace(/\s+/g, " ");
    index.push({ kind: "page", label: x.f.label, hint: x.f.group, id: x.f.id, text });
    for (const [h, l] of x.p.toc) index.push({ kind: "section", label: l, hint: x.f.label, id: x.f.id, anchor: h });
  }
  for (const [l, id] of API) index.push({ kind: "api", label: l, hint: byId(id).label, id });
  return index;
}
function search(q) {
  const t = q.trim().toLowerCase();
  if (!index) return [];
  if (!t) return [...index.filter(x => x.kind === "page").slice(0, 4), ...index.filter(x => x.kind === "api").slice(0, 4)];
  const score = x => {
    const l = x.label.toLowerCase();
    if (l === t) return 0; if (l.startsWith(t)) return 1; if (l.includes(t)) return 2;
    if (x.kind === "page" && x.text.toLowerCase().includes(t)) return 3;
    return -1;
  };
  return index.map(x => ({ x, s: score(x) })).filter(r => r.s >= 0).sort((a, b) => a.s - b.s).map(r => r.x).slice(0, 12);
}
function snippet(text, t) {
  const i = text.toLowerCase().indexOf(t);
  if (i < 0) return null;
  return "…" + text.slice(Math.max(0, i - 20), i + t.length + 30).trim() + "…";
}
function renderResults() {
  const q = $("#q").value, t = q.trim().toLowerCase();
  const res = search(q);
  const groups = [["page", "Pages"], ["section", "Sections"], ["api", "API & terms"]];
  shown = groups.flatMap(([k]) => res.filter(r => r.kind === k));
  sel = Math.min(sel, Math.max(shown.length - 1, 0));
  const mark = s => t ? esc(s).replace(new RegExp(esc(t).replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "ig"), m => `<mark>${m}</mark>`) : esc(s);
  let n = 0;
  const row = x => {
    const snip = x.kind === "page" && t && !x.label.toLowerCase().includes(t) ? snippet(x.text, t) : null;
    const icon = x.kind === "page" ? I.page : x.kind === "section" ? I.hash : I.code;
    return `<div class="palette-item ${n === sel ? "sel" : ""}" data-n="${n++}">${icon}<span>${mark(x.label)}</span>
      <small class="${snip ? "snip" : ""}">${snip ? mark(snip) : esc(x.hint)}</small></div>`;
  };
  const html = groups.map(([k, title]) => {
    const g = shown.filter(r => r.kind === k);
    return g.length ? `<div class="palette-group">${title}</div>${g.map(row).join("")}` : "";
  }).join("");
  $("#results").innerHTML = html || (index ? `<div class="palette-item">No results for “${esc(q)}”</div>` : `<div class="palette-item">Loading…</div>`);
  document.querySelectorAll(".palette-item[data-n]").forEach(el => {
    el.onclick = () => go(shown[+el.dataset.n]);
    el.onmousemove = () => { if (sel !== +el.dataset.n) { sel = +el.dataset.n; document.querySelectorAll(".palette-item").forEach(e => e.classList.toggle("sel", e === el)); } };
  });
  $("#results").querySelector(".sel")?.scrollIntoView({ block: "nearest" });
}
async function openSearch() {
  $("#overlay").classList.add("open"); $("#q").value = ""; sel = 0;
  renderResults(); setTimeout(() => $("#q").focus(), 10);
  await buildIndex(); renderResults();
}
function closeSearch() { $("#overlay").classList.remove("open"); }
function go(x) { closeSearch(); location.hash = route(x.id, x.anchor); }

// ───────── Boot ─────────
renderShell();
let saved = null; try { saved = localStorage.getItem("theme"); } catch {}
setTheme(new URLSearchParams(location.search).get("theme") || saved || "dark");
$("#theme").onclick = () => setTheme(document.documentElement.dataset.theme === "light" ? "dark" : "light");
$("#menu").onclick = () => document.body.classList.toggle("nav-open");
$("#scrim").onclick = () => document.body.classList.remove("nav-open");
$("#search").onclick = openSearch;
$("#overlay").onclick = e => { if (e.target.id === "overlay") closeSearch(); };
$("#q").oninput = () => { sel = 0; renderResults(); };
document.addEventListener("keydown", e => {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") { e.preventDefault(); openSearch(); return; }
  if (e.key === "/" && !$("#overlay").classList.contains("open") && !/input|textarea/i.test(document.activeElement.tagName)) { e.preventDefault(); openSearch(); return; }
  if (!$("#overlay").classList.contains("open")) return;
  if (e.key === "Escape") closeSearch();
  else if (e.key === "ArrowDown") { e.preventDefault(); sel = Math.min(sel + 1, shown.length - 1); renderResults(); }
  else if (e.key === "ArrowUp") { e.preventDefault(); sel = Math.max(sel - 1, 0); renderResults(); }
  else if (e.key === "Enter" && shown[sel]) go(shown[sel]);
});
window.addEventListener("hashchange", renderPage);
renderPage();
