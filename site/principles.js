// supragnosis principles page (docs/design-system.md Section 11a, U12-U14): the ledger's columns, the
// five demos, the nineteen scenes and the tension map. Every scene is a pure function of the time,
// drawn with the viewer's marks and the landing's tokens; the words are the page's, and every value
// drawn as the product's behavior is the cases' at the end of the page, which tests replay. The
// script holds no resolution rule.
(function () {
"use strict";
const $ = (s, r) => (r || document).querySelector(s);
const $$ = (s, r) => Array.from((r || document).querySelectorAll(s));
function el(tag, attrs, text) { const e = document.createElement(tag); if (attrs) for (const k in attrs) e.setAttribute(k, attrs[k]); if (text != null) e.textContent = text; return e; }
// Reduced motion is read when a scene starts, so a change of the setting is followed (Section 11a).
const RM = matchMedia("(prefers-reduced-motion: reduce)");
// On a phone a scene does not play only because it scrolled into view (design-system.md Section 11a):
// scrolling past is not asking to watch, so its button says Play scene until it is pressed.
const PHONE = matchMedia("(max-width: 559px)");
const clamp = (x, a, b) => Math.max(a, Math.min(b, x));
const ease = x => { x = clamp(x, 0, 1); return x < .5 ? 2 * x * x : 1 - Math.pow(-2 * x + 2, 2) / 2; };

// ---- the page's words, in English and Korean (docs/i18n.md Section 4): what a figure draws and the
// controls the script makes, keyed by the page's language. A name, a type, an id, a command and what
// an agent reads stay as written (Section 2), and a value from the cases is said through `val`. ----
const LANG = document.documentElement.lang === "ko" ? "ko" : "en";
const WORDS = {
  // the controls
  steps: { en: "Steps", ko: "단계" },
  playScene: { en: "Play scene", ko: "장면 재생" },
  replay: { en: "Replay", ko: "다시 재생" },
  stop: { en: "Stop", ko: "정지" },
  readPrinciple: { en: "Read the principle", ko: "원칙 읽기" },
  principles: { en: "Principles", ko: "원칙" },
  allPrinciples: { en: "All principles", ko: "모든 원칙" },
  whereOf: { en: "P{p} of {n}", ko: "P{p} / {n}" },
  previous: { en: "Previous", ko: "이전" },
  next: { en: "Next", ko: "다음" },
  // words more than one figure draws
  owed: { en: "owed", ko: "남은 과제" },
  you: { en: "you", ko: "나" },
  hub: { en: "hub", ko: "허브" },
  graph: { en: "graph", ko: "그래프" },
  graphFp: { en: "graph {fp}", ko: "그래프 {fp}" },
  sameThing: { en: "same thing?", ko: "같은 것?" },
  merged: { en: "merged", ko: "병합됨" },
  contested: { en: "contested", ko: "이견 있음" },
  log: { en: "log", ko: "로그" },
  // the values the cases hold, by their English
  tierHuman: { en: "human confirmed", ko: "사람이 확인" },
  tierHost: { en: "host signed", ko: "호스트 서명" },
  tierAgent: { en: "agent extracted", ko: "에이전트" },
  unstated: { en: "unstated", ko: "명시 안 됨" },
  // P16
  nOf6: { en: "{n} of 6", ko: "{n} / 6" },
  dupArrived: { en: "a duplicate arrived, nothing changed", ko: "중복 도착, 바뀐 것 없음" },
  linkCut: { en: "link cut", ko: "연결 끊김" },
  identical: { en: "identical", ko: "동일" },
  // P6
  laptopForYou: { en: "laptop, for you", ko: "laptop, 나를 위해" },
  redisDatabase: { en: "Redis is a Database", ko: "Redis는 Database" },
  redisCache: { en: "Redis is a Cache", ko: "Redis는 Cache" },
  believed: { en: "believed", ko: "믿음" },
  believedTrust: { en: "believed, by trust", ko: "신뢰로 믿음" },
  defeatedKept: { en: "defeated, kept", ko: "밀려남, 보존" },
  youConsole: { en: "you, in the console", ko: "나, 콘솔에서" },
  confirmType: { en: "confirm \"{type}\"", ko: "\"{type}\" 확인" },
  typeContested: { en: "{type}, contested", ko: "{type}, 이견 있음" },
  // P18
  desk2Sent: { en: "what desk-2 sent", ko: "desk-2가 보낸 것" },
  nodeKeeps: { en: "what your node keeps", ko: "내 노드가 보관하는 것" },
  entryVerdict: { en: "entry + a console verdict", ko: "항목 + 콘솔 판정" },
  claimed: { en: "claimed", ko: "주장" },
  evaluated: { en: "evaluated", ko: "평가" },
  verdict: { en: "verdict", ko: "판정" },
  storedAsSent: { en: "stored as sent", ko: "보낸 그대로 저장" },
  signedNoVerdict: { en: "signed by desk-2, no verdict behind it", ko: "desk-2의 서명, 뒤에 판정 없음" },
  personMerged: { en: "a person merged a promotion in desk-2's console", ko: "desk-2의 콘솔에서 사람이 승격을 병합함" },
  console: { en: "console", ko: "콘솔" },
  consoleReplayed: { en: "the console verdict, replayed from the log", ko: "로그에서 다시 재생한 콘솔 판정" },
  // P23 and P5
  pgForwards: { en: "pg forwards here", ko: "pg의 전달 대상" },
  thisNode: { en: "this node", ko: "이 노드" },
  // P1 and P2
  noGraph: { en: "no graph", ko: "그래프 없음" },
  sameAsBefore: { en: "same as before", ko: "이전과 같음" },
  fHost: { en: "host", ko: "호스트" },
  fFor: { en: "on behalf of", ko: "위임자" },
  fWorkspace: { en: "workspace", ko: "워크스페이스" },
  fObserved: { en: "observed", ko: "관찰 시각" },
  fSource: { en: "source", ko: "출처" },
  fConfidence: { en: "confidence", ko: "확신도" },
  hit: { en: "hit: billing-api uses Postgres", ko: "결과: billing-api uses Postgres" },
  // P3 and P4
  entryInLog: { en: "{n} entry in the log", ko: "로그에 항목 {n}개" },
  entriesInLog: { en: "{n} entries in the log", ko: "로그에 항목 {n}개" },
  superseded: { en: "superseded", ko: "대체됨" },
  keptOnce: { en: "kept once", ko: "한 번만 보관" },
  destroy: { en: "destroy", ko: "파기" },
  trueWorld: { en: "true in the world", ko: "세상에서 참인 때" },
  knownSystem: { en: "known to the system", ko: "시스템이 알게 된 때" },
  usesPgSince: { en: "uses Postgres, since {date}", ko: "Postgres 사용, {date}부터" },
  learnedOn: { en: "learned {date}", ko: "{date}에 알게 됨" },
  trueNotKnown: { en: "true, not yet known", ko: "참, 아직 모름" },
  validOn: { en: "valid {date}", ko: "유효 {date}" },
  recordedOn: { en: "recorded {date}", ko: "기록 {date}" },
  asOf: { en: "as of {date}?", ko: "{date} 기준?" },
  // P7 and P8
  idleReads: { en: "idle: consolidation reads the log", ko: "유휴: 통합이 로그를 읽음" },
  mergePg: { en: "merge pg into Postgres?", ko: "pg를 Postgres에 병합?" },
  promoteContext: { en: "promote this context?", ko: "이 맥락을 승격?" },
  canonZero: { en: "canon: 0 changes", ko: "정본: 변경 0건" },
  retrieval: { en: "retrieval", ko: "검색" },
  vocabulary: { en: "vocabulary", ko: "어휘" },
  defineCache: { en: "define Cache", ko: "Cache 정의" },
  needsDefinition: { en: "needs a definition", ko: "정의가 필요함" },
  descBlank: { en: "description: blank", ko: "설명: 비어 있음" },
  kept: { en: "kept", ko: "보존" },
  // P9 and P10
  relationType: { en: "relation type: member_of", ko: "관계 타입: member_of" },
  addType: { en: "add type member_of", ko: "타입 member_of 추가" },
  blocked: { en: "blocked", ko: "차단됨" },
  subtypeCycles: { en: "subtype cycles, domain and range", ko: "하위 타입 순환, 정의역과 치역" },
  yourVocabulary: { en: "your vocabulary", ko: "내 어휘" },
  coreModel: { en: "core model", ko: "핵심 모델" },
  migration: { en: "migration: v1 entries read as v2", ko: "마이그레이션: v1 항목을 v2로 읽음" },
  fromLog: { en: "from the log", ko: "로그에서" },
  // P11 and P12
  saidTogether: { en: "said together {n}x", ko: "함께 언급 {n}회" },
  derivedLowest: { en: "derived, lowest trust", ko: "파생, 가장 낮은 신뢰" },
  typeCandidate: { en: "type candidate?", ko: "타입 후보?" },
  storeIs: { en: "store: {s}", ko: "저장소: {s}" },
  port: { en: "port", ko: "포트" },
  btreePage: { en: "B-tree page", ko: "B-tree 페이지" },
  // P13, P14 and P15
  notSubtype: { en: "not a subtype", ko: "하위 타입 아님" },
  sameId: { en: "same id", ko: "같은 id" },
  oneEntry: { en: "1 entry, counted once", ko: "항목 1개, 한 번만 셈" },
  getPgForwarded: { en: "get pg: forwarded", ko: "pg 조회: 전달됨" },
  split: { en: "split?", ko: "분리?" },
  proposal: { en: "proposal", ko: "제안" },
  splitProposal: { en: "split proposal", ko: "분리 제안" },
  // P17 and P19
  yourLaptop: { en: "your laptop", ko: "내 노트북" },
  teamHub: { en: "team hub", ko: "팀 허브" },
  shared: { en: "shared", ko: "공유됨" },
  refusedCredential: { en: "refused: shaped like a credential", ko: "거부됨: 자격 증명처럼 생김" },
  searchModel: { en: "search model", ko: "검색 모델" },
  searchModelOff: { en: "search model: off", ko: "검색 모델: 꺼짐" },
  keywordOnly: { en: "keyword only", ko: "키워드만" },
  waitsVerdict: { en: "waits for a verdict", ko: "판정을 기다림" },
  written: { en: "written", ko: "기록됨" },
  // P20, P21 and P22
  inMemory: { en: "in-memory", ko: "인메모리" },
  aModel: { en: "a model", ko: "모델" },
  tools13: { en: "13 tools", ko: "도구 13개" },
  observation9b1e: { en: "observation 9b1e", ko: "관찰 9b1e" },
  noPassthrough: { en: "no passthrough", ko: "그대로 전달하지 않음" },
  newSession: { en: "-- a new session --", ko: "-- 새 세션 --" },
  // P24
  serving: { en: "serving", ko: "서비스 중" },
  refusedStart: { en: "refused to start", ko: "시작 거부" },
  rStore: { en: "store", ko: "저장소" },
  rKeyword: { en: "keyword search", ko: "키워드 검색" },
  rSemantic: { en: "semantic search", ko: "의미 검색" },
  rSync: { en: "sync", ko: "동기화" },
  rViewer: { en: "viewer", ko: "뷰어" },
  offModel: { en: "off: model not fetched; keyword only", ko: "꺼짐: 모델을 받지 않음, 키워드만" },
  offHub: { en: "off: hub unreachable; see sync_status", ko: "꺼짐: 허브에 닿지 않음, sync_status 참고" },
  configKey: { en: "config: a key this build does not know", ko: "설정: 이 빌드가 모르는 키" },
};
const word = (key, v) => WORDS[key][LANG].replace(/\{(\w+)\}/g, (m, k) => v && k in v ? v[k] : m);
// A value the cases hold is drawn as they hold it, and said in the page's language.
const VALUE = Object.fromEntries(["tierHuman", "tierHost", "tierAgent", "unstated"].map(k => [WORDS[k].en, k]));
const val = v => VALUE[v] ? word(VALUE[v]) : v;
// Plurals, months, dates and days through Intl, in the page's language (docs/i18n.md Section 4); the made-up
// project runs in 2026, and its Monday is January 5.
const INTL = LANG === "ko" ? "ko" : "en-US";
const PLURAL = new Intl.PluralRules(INTL);
const MONTH = new Intl.DateTimeFormat(INTL, {month: "short", timeZone: "UTC"});
const DATE = new Intl.DateTimeFormat(INTL, {month: "short", day: "numeric", timeZone: "UTC"});
const WEEKDAY = new Intl.DateTimeFormat(INTL, {weekday: "short", timeZone: "UTC"});
const month = i => MONTH.format(Date.UTC(2026, i, 1));
const date = (i, d) => DATE.format(Date.UTC(2026, i, d));
const day = (i, hm) => WEEKDAY.format(Date.UTC(2026, 0, 5 + i)) + " " + hm;

// ---- the viewer's marks: its color functions copied character for character (a test compares) ----
// The landing's tokens (site/style.css), one value each; a test holds them equal.
const C = {
  bg: "#10131b",       // --panel, a canvas's ground and every label's halo
  well: "#161a23",     // --panel-2, a log, a panel inside a scene, an owed tag
  panel3: "#1d222d",   // --panel-3, a chip
  select: "#2a2826",   // --select, a chip that is chosen
  ink: "#e9e4d6",      // --ink
  ink2: "#cad2de",     // --ink2
  muted: "#b0b8c7",    // --muted
  gold: "#d9a544",     // --accent, a proposal waiting for a verdict
  goldHi: "#f0c469",   // --gold-bright, a person's confirmation
  teal: "#56b3a2",     // --ok, an agent at work
  bad: "#e07a5f",      // --bad, contested, refused, blocked
};
const EDGE_NEUTRAL = "#6b7383", EDGE_ALPHA = 0.42;
const SANS = '"Pretendard Variable", Pretendard, -apple-system, "Segoe UI", sans-serif';
// Plex Mono has no Hangul: on a Korean page a mono label's Hangul is Pretendard's, not a system face's
const MONO = LANG === "ko" ? '"IBM Plex Mono", "Pretendard Variable", ui-monospace, Menlo, monospace' : '"IBM Plex Mono", ui-monospace, Menlo, monospace';
const TYPE_BAND = [
  "#889d37", "#82d48c", "#3cd1f5", "#67acf7", "#8986de", "#c38fe3", "#c670ab", "#ff9abd",
  "#adc35e", "#53a45e", "#009cba", "#4c8fd8", "#aeadff", "#aa76c8", "#ffb7e6", "#e17da1",
];
const BAND_HUES = 8;
function nameSlot(name) {
  let h = 2166136261;   // FNV-1a
  for (let i = 0; i < name.length; i++) { h ^= name.charCodeAt(i); h = Math.imul(h, 16777619) >>> 0; }
  return h % BAND_HUES;
}
function stableColors(names) {
  const out = {}, used = new Set();
  for (const t of [...names].sort()) {
    const pref = nameSlot(t);
    let slot = pref;
    for (let k = 0; k < TYPE_BAND.length; k++) {
      const s = (k < BAND_HUES ? 0 : BAND_HUES) + (pref + k) % BAND_HUES;
      if (!used.has(s)) { slot = s; break; }
    }
    used.add(slot);
    out[t] = TYPE_BAND[slot];
  }
  return out;
}
const TYPE = stableColors(["App", "Database", "Feature", "Pattern", "Event", "Cache"]);
const CONCEPT = C.muted;
// A rounded box; a box with no width or height draws nothing, and its corners never exceed its sides.
function rrect(ctx, x, y, w, h, r) {
  ctx.beginPath();
  if (!(w > 0 && h > 0)) return;
  r = Math.max(0, Math.min(r, w / 2, h / 2));
  if (ctx.roundRect) ctx.roundRect(x, y, w, h, r); else ctx.rect(x, y, w, h);
}
// The contested badge's mark (docs/design-system.md Section 28.3, U41): Lucide's circle-alert bar and
// dot, in its own 24-unit grid, with the badge's disc of radius r taken as the icon's 10-unit circle -
// the set's 2-unit stroke, never under 1.5 pixels. The viewer draws the same mark.
const ALERT_MARK = new Path2D("M12 8v4M12 16h.01");
function alertMark(ctx, x, y, r, color) {
  const k = r / 10;
  ctx.save();
  ctx.translate(x - 12 * k, y - 12 * k); ctx.scale(k, k);
  ctx.lineCap = "round"; ctx.strokeStyle = color; ctx.lineWidth = Math.max(2, 1.5 / k);
  ctx.stroke(ALERT_MARK);
  ctx.restore();
}
function label(ctx, s, x, y, o) {
  o = o || {};
  ctx.globalAlpha = o.alpha == null ? 1 : o.alpha;
  ctx.font = `${o.weight || 500} ${o.size || 12}px ${o.mono ? MONO : SANS}`;
  ctx.textAlign = o.align || "left"; ctx.textBaseline = "middle";
  if (o.halo !== false) { ctx.lineJoin = "round"; ctx.lineWidth = 4; ctx.strokeStyle = C.bg; ctx.strokeText(s, x, y); }
  ctx.fillStyle = o.color || C.ink2; ctx.fillText(s, x, y);
  ctx.globalAlpha = 1;
}
function dot(ctx, x, y, r, fill, o) {
  o = o || {};
  if (r <= .2) return;
  ctx.globalAlpha = o.alpha == null ? 1 : o.alpha;
  ctx.beginPath(); ctx.arc(x, y, r, 0, 7); ctx.fillStyle = fill; ctx.fill();
  ctx.lineWidth = clamp(r * .35, 1.5, 4); ctx.strokeStyle = C.bg; ctx.stroke();
  const p = o.ringP == null ? 1 : o.ringP;
  if (o.ring && p > 0) {
    ctx.beginPath(); ctx.arc(x, y, r + 3.5, -Math.PI / 2, -Math.PI / 2 + Math.PI * 2 * p);
    ctx.lineWidth = o.ring === "gold" ? 2.2 : 1.2; ctx.strokeStyle = o.ring === "gold" ? C.goldHi : C.ink; ctx.stroke();
  }
  if (o.bad > 0) {
    const bx = x + r * .75, by = y - r * .75;
    ctx.globalAlpha = Math.min(1, o.bad);
    ctx.beginPath(); ctx.arc(bx, by, 7 * o.bad, 0, 7); ctx.fillStyle = C.bad; ctx.fill(); ctx.lineWidth = 1.5; ctx.strokeStyle = C.bg; ctx.stroke();
    if (o.bad > .6) alertMark(ctx, bx, by, 7, C.bg);
  }
  ctx.globalAlpha = 1;
}
function line(ctx, a, b, o) {
  o = o || {};
  const p = o.p == null ? 1 : o.p;
  if (p <= 0) return;
  ctx.globalAlpha = o.alpha == null ? EDGE_ALPHA : o.alpha;
  ctx.strokeStyle = o.color || EDGE_NEUTRAL; ctx.lineWidth = o.w || 1.2; ctx.lineCap = "round";
  ctx.beginPath(); ctx.moveTo(a[0], a[1]); ctx.lineTo(a[0] + (b[0] - a[0]) * p, a[1] + (b[1] - a[1]) * p); ctx.stroke();
  ctx.globalAlpha = 1;
}
function arrow(ctx, a, b, ra, rb, al) {
  // the viewer's merge preview: the name folded away points into the one kept
  const dx = b[0] - a[0], dy = b[1] - a[1], d = Math.hypot(dx, dy) || 1, ux = dx / d, uy = dy / d;
  const tx = b[0] - ux * (rb + 5), ty = b[1] - uy * (rb + 5), hl = clamp(rb * .9, 6, 10), hw = hl * .55;
  ctx.globalAlpha = al; ctx.strokeStyle = C.gold; ctx.fillStyle = C.gold; ctx.lineWidth = 2;
  ctx.beginPath(); ctx.moveTo(a[0] + ux * (ra + 4), a[1] + uy * (ra + 4)); ctx.lineTo(tx, ty); ctx.stroke();
  ctx.beginPath(); ctx.moveTo(tx, ty); ctx.lineTo(tx - ux * hl - uy * hw, ty - uy * hl + ux * hw); ctx.lineTo(tx - ux * hl + uy * hw, ty - uy * hl - ux * hw); ctx.closePath(); ctx.fill();
  ctx.globalAlpha = 1;
}
function surface(cv, heightOf, draw) {
  const ctx = cv.getContext("2d"), s = {ctx, W: 0, H: 0, dpr: 1, away: false};
  function size() {
    const w = cv.getBoundingClientRect().width || cv.parentElement.getBoundingClientRect().width || 600;
    s.W = w; s.H = Math.round(heightOf(w)); cv.style.height = s.H + "px";
    s.dpr = Math.min(2, window.devicePixelRatio || 1);
    cv.width = Math.max(1, Math.round(w * s.dpr)); cv.height = Math.max(1, Math.round(s.H * s.dpr));
  }
  s.redraw = () => { if (s.away) return; ctx.setTransform(s.dpr, 0, 0, s.dpr, 0, 0); ctx.clearRect(0, 0, s.W, s.H); ctx.globalAlpha = 1; draw(s); };
  size();
  new ResizeObserver(() => { const w = cv.getBoundingClientRect().width; if (!s.away && Math.abs(w - s.W) > .5) { size(); s.redraw(); } }).observe(cv);
  new IntersectionObserver(es => {
    const near = es.some(e => e.isIntersecting);
    if (!near && !s.away) { s.away = true; cv.width = cv.height = 1; }
    else if (near && s.away) { s.away = false; size(); s.redraw(); }
  }, {rootMargin: "1600px 0px"}).observe(cv);
  // a change of pixel ratio, as when a window moves to another screen, redraws at the new density
  matchMedia(`(resolution: ${window.devicePixelRatio || 1}dppx)`).addEventListener("change", () => { if (!s.away) { size(); s.redraw(); } }, {once: true});
  return s;
}
function onceVisible(elm, fn) {
  if (!("IntersectionObserver" in window)) return;
  const io = new IntersectionObserver(es => { if (es.some(e => e.isIntersecting)) { io.disconnect(); fn(); } }, {threshold: .5});
  io.observe(elm);
}
// Every running scene, so a hidden tab can stop them all at their resting frames.
const RUNNING = new Set();
function animate(dur, frame, done) {
  const finish = () => { RUNNING.delete(stopper); frame(Infinity); if (done) done(); };
  if (RM.matches) { frame(Infinity); if (done) done(); return () => {}; }
  let raf = 0; const t0 = performance.now();
  const step = now => { const t = (now - t0) / 1000; if (t < dur) { frame(t); raf = requestAnimationFrame(step); } else finish(); };
  const stopper = () => { cancelAnimationFrame(raf); finish(); };
  RUNNING.add(stopper);
  raf = requestAnimationFrame(step);
  return () => { cancelAnimationFrame(raf); RUNNING.delete(stopper); };
}
document.addEventListener("visibilitychange", () => { if (document.hidden) [...RUNNING].forEach(f => f()); });
const MOUNT = {};
// A demo's sentences are the page's (Section 11a); the script picks the one for the state it draws.
// Copied as nodes, never as markup: the page's own words are moved, not reparsed.
const cloneKids = node => { const f = document.createDocumentFragment(); if (node) node.childNodes.forEach(n => f.append(n.cloneNode(true))); return f; };
const capOf = (fig, k) => cloneKids($(`.caps li[data-k="${k}"]`, fig));
const capText = (fig, k) => { const li = $(`.caps li[data-k="${k}"]`, fig); return li ? li.textContent : ""; };
// A demo's row: who said it (one line each), what, and the state it is in.
function rowEl(who, what, chip, cls, dim) {
  const r = el("div", dim ? {class: "dim"} : null), w = el("span", {class: "who"});
  who.forEach((line, i) => { if (i) w.append(el("br")); w.append(line); });
  r.append(w, el("span", {class: "what"}, what), el("span", {class: ("chip " + cls).trim()}, chip));
  return r;
}
// The values a demo or scene presents as the product's behavior are the cases' (U12).
const CASES = (() => { try { return JSON.parse($("#cases").textContent); } catch (e) { return {}; } })();
// A figure's case, keyed by its principle: the values the figure draws as the product's behavior.
const V = p => (CASES["p" + p] && CASES["p" + p].values) || {};
const FP = V(1).digest || "";

// ---- P16: same log, same graph ----
MOUNT[16] = function (fig) {
  const cv = $("canvas", fig), cap = $(".cap", fig);
  const ENT = [
    {n: [["api", "App"]], e: []},
    {n: [["pg", "Database"]], e: []},
    {n: [["api", null], ["pg", null]], e: [["api", "pg"]]},
    {n: [["wh", "Feature"]], e: []},
    {n: [["api", null], ["wh", null]], e: [["api", "wh"]]},
    {n: [["wh", null], ["idem", "Pattern"]], e: [["wh", "idem"]]},
  ];
  const POS = {api: [.22, .34], pg: [.78, .18], wh: [.66, .64], idem: [.26, .86]};
  const NAME = {api: "billing-api", pg: "Postgres", wh: "webhooks", idem: "idempotency"};
  const MACH = ["laptop", "desktop", word("hub")];
  let seed = 3, mode = "plain", plan = [], t = Infinity, stop = () => {};
  function rnd(a) { return () => { a |= 0; a = a + 0x6D2B79F5 | 0; let x = Math.imul(a ^ a >>> 15, 1 | a); x = x + Math.imul(x ^ x >>> 7, 61 | x) ^ x; return ((x ^ x >>> 14) >>> 0) / 4294967296; }; }
  function build() {
    const r = rnd(seed * 7919);
    plan = MACH.map((name, i) => {
      const order = [0, 1, 2, 3, 4, 5];
      for (let k = 5; k > 0; k--) { const j = (r() * (k + 1)) | 0; [order[k], order[j]] = [order[j], order[k]]; }
      const arr = order.map((k, j) => ({k, at: .35 + j * .5 + i * .17 + r() * .1}));
      if (mode === "dup" && i === 1) arr.push({k: arr[2].k, at: arr[5].at + .45, dup: true});
      if (mode === "cut" && i === 2) arr.forEach((a, j) => { if (j >= 2) a.at = 3.9 + (j - 2) * .2; });
      return {name, arr};
    });
  }
  const end = () => Math.max(...plan.flatMap(p => p.arr.map(a => a.at))) + .8;
  function stateOf(p, t) {
    const got = new Map(); let dupAt = -1;
    for (const a of p.arr) if (a.at <= t) { if (got.has(a.k)) dupAt = a.at; else got.set(a.k, a.at); }
    const nodes = {}, edges = {};
    for (const [k, at] of got) {
      const e = ENT[k];
      for (const [id, kind] of e.n) { const n = nodes[id] || (nodes[id] = {born: at, kind: null, kindAt: 0}); if (at < n.born) n.born = at; if (kind) { n.kind = kind; n.kindAt = at; } }
      for (const [a, b] of e.e) { const key = a + ">" + b; if (!(key in edges) || at < edges[key]) edges[key] = at; }
    }
    return {got, nodes, edges, dupAt};
  }
  function rects(W, H) {
    const g = 10;
    if (W < 560) { const h = (H - 2 * g) / 3; return [0, 1, 2].map(i => [0, i * (h + g), W, h]); }
    const w = (W - 2 * g) / 3; return [0, 1, 2].map(i => [i * (w + g), 0, w, H]);
  }
  function draw(s) {
    const {ctx, W, H} = s, R = rects(W, H);
    // every machine holding all six entries holds the same set, and the cases replay that set in
    // these orders against the engine (U12); the picture claims nothing about a partial set
    const sts = plan.map(p => stateOf(p, t)), fps = sts.map(() => FP), same = sts.every(st => st.got.size === 6);
    plan.forEach((p, i) => panel(ctx, R[i], p, sts[i], fps[i], same, i));
  }
  function panel(ctx, rc, p, st, f, same, i) {
    const [x, y, w, h] = rc, flat = w > h * 1.6, lw = flat ? w * .44 : w;
    ctx.fillStyle = C.bg; rrect(ctx, x, y, w, h, 10); ctx.fill();
    label(ctx, p.name, x + 12, y + 18, {color: C.ink, weight: 600, size: 12.5, halo: false});
    label(ctx, word("nOf6", {n: st.got.size}), x + lw - 12, y + 18, {align: "right", mono: true, size: 11, color: C.muted, halo: false});
    const wx = x + 12, wy = y + 32, ww = lw - 24, wh = 18, sw = ww / 6;
    ctx.fillStyle = C.well; rrect(ctx, wx, wy, ww, wh, 5); ctx.fill();
    let j = 0;
    for (const a of p.arr) {
      if (a.dup) continue;
      if (a.at <= t) {
        const cx = wx + sw * (j + .5), fl = 1 - clamp((t - a.at) / .6, 0, 1);
        ctx.globalAlpha = .6 + .4 * fl; ctx.fillStyle = fl > .02 ? C.ink : C.ink2;
        rrect(ctx, cx - sw * .38, wy + 3, sw * .76, wh - 6, 3); ctx.fill(); ctx.globalAlpha = 1;
        label(ctx, String(a.k + 1), cx, wy + wh / 2 + .5, {align: "center", mono: true, size: 10, weight: 600, color: C.bg, halo: false});
      }
      j++;
    }
    const ny = wy + wh + 13;
    if (st.dupAt >= 0 && t - st.dupAt < 1.6) label(ctx, word("dupArrived"), wx, ny, {size: 11, color: C.muted, halo: false});
    if (mode === "cut" && i === 2 && t > 1.3 && t < 3.9) label(ctx, word("linkCut"), wx, ny, {size: 11, color: C.bad, halo: false, mono: true});
    const gx = flat ? x + lw : x + 14, gy = flat ? y + 14 : wy + wh + 28;
    const gw = flat ? w - lw - 18 : w - 28, gh = flat ? h - 28 : (y + h - 34) - gy;
    const P = id => [gx + POS[id][0] * gw, gy + POS[id][1] * gh];
    const r = clamp(Math.min(gw, gh * 1.6) * .045, 5, 10);
    for (const key in st.edges) { const [a, b] = key.split(">"); line(ctx, P(a), P(b), {p: ease((t - st.edges[key]) / .45)}); }
    const showLabels = gw > 150;
    for (const id in st.nodes) {
      const n = st.nodes[id], sc = ease((t - n.born) / .35), [px, py] = P(id);
      const kk = n.kind ? ease((t - n.kindAt) / .4) : 0;
      if (kk < 1) dot(ctx, px, py, r * sc, CONCEPT);
      if (kk > 0) dot(ctx, px, py, r * sc, TYPE[n.kind], {alpha: kk});
      if (showLabels && sc > .3) { const left = POS[id][0] > .6; label(ctx, NAME[id], left ? px - r - 6 : px + r + 6, py, {align: left ? "right" : "left", size: 11.5, alpha: sc}); }
    }
    label(ctx, st.got.size === 6 ? word("graphFp", {fp: FP}) : word("graph"), x + 12, y + h - 15, {mono: true, size: 11, color: same ? C.ink : C.muted, halo: false});
    if (same) label(ctx, word("identical"), x + lw - 12, y + h - 15, {align: "right", size: 11, color: C.ink, halo: false});
  }
  function capRunning() { cap.replaceChildren(capOf(fig, mode)); }
  function capDone() {
    const orders = plan.map(p => p.arr.filter(a => !a.dup).map(a => a.k + 1).join(" ")).join(" / ");
    const [a, rest] = capText(fig, "done").split("{orders}"), [b, c] = (rest || "").split("{fp}");
    cap.replaceChildren(a + orders + (b || ""), el("code", null, FP), (c || "") + (mode === "plain" ? "" : " " + capText(fig, "done-" + mode)));
  }
  function run() { stop(); capRunning(); stop = animate(end(), tt => { t = tt; S.redraw(); }, capDone); }
  build();
  const S = surface(cv, w => w < 560 ? 3 * 178 + 20 : clamp(w * .44, 270, 360), draw);
  $$("button", fig).forEach(b => b.addEventListener("click", () => { const a = b.dataset.a; if (a === "shuffle") { seed++; mode = "plain"; } else mode = a; build(); run(); }));
  t = Infinity; S.redraw(); capDone();
  onceVisible(fig, run);
};

// ---- P6: disagreement surfaces ----
MOUNT[6] = function (fig) {
  const cv = $("canvas", fig), rows = $(".rows", fig), cap = $(".cap", fig);
  const bReplay = $('[data-a="replay"]', fig), bConfirm = $('[data-a="confirm"]', fig);
  let step = 1, from = 1, k = 1, stop = () => {}, timer = 0;
  const BELIEF = [V(6).first, V(6).during, V(6).after];
  const S = surface(cv, w => clamp(w * .46, 210, 280), draw);
  function draw(s) {
    const {ctx, W, H} = s, R = [W * .5, H * .34], A = [W * .2, H * .7], B = [W * .8, H * .7];
    const r = clamp(W * .03, 10, 15), rc = 7, kk = from === step ? 1 : ease(k);
    const kB = step >= 1 ? (from >= 1 ? 1 : kk) : 0, kV = step === 2 ? (from === 2 ? 1 : kk) : 0;
    const contested = step === 1 ? (from === 0 ? kk : 1) : step === 2 && from === 1 ? 1 - kk : 0;
    line(ctx, A, R, {alpha: .42 + .5 * kV, color: kV > .5 ? C.ink : EDGE_NEUTRAL, w: kV > .5 ? 1.6 : 1.2});
    if (kB > 0) line(ctx, B, R, {alpha: .42 * kB * (1 - .5 * kV)});
    dot(ctx, A[0], A[1], rc, TYPE.Database, {ring: kV > 0 ? "gold" : null, ringP: kV});
    label(ctx, "laptop: Database", A[0], A[1] + 21, {align: "center", size: 11.5});
    if (kB > 0) {
      dot(ctx, B[0], B[1], rc * kB, TYPE.Cache, {alpha: 1 - .55 * kV});
      label(ctx, "ci-runner: Cache", B[0], B[1] + 21, {align: "center", size: 11.5, alpha: kB * (1 - .3 * kV)});
      if (kV > 0) label(ctx, word("defeatedKept"), B[0], B[1] + 37, {align: "center", size: 11, mono: true, color: C.muted, alpha: kV});
    }
    const prevB = BELIEF[from], nowB = BELIEF[step];
    dot(ctx, R[0], R[1], r, TYPE[prevB]);
    if (prevB !== nowB) dot(ctx, R[0], R[1], r, TYPE[nowB], {alpha: kk});
    if (kV > 0 || contested > 0) dot(ctx, R[0], R[1], r, TYPE[kk > .5 ? nowB : prevB], {ring: kV > 0 ? "gold" : null, ringP: kV, bad: contested, alpha: 1});
    label(ctx, "Redis", R[0] + r + 10, R[1] - 1, {size: 13, weight: 600, color: C.ink});
    label(ctx, contested > .5 ? word("typeContested", {type: kk > .5 ? nowB : prevB}) : kk > .5 ? nowB : prevB, R[0] + r + 10, R[1] + 16, {size: 11, mono: true, color: contested > .5 ? C.bad : C.muted});
  }
  function render() {
    const r = [];
    r.push(rowEl([word("laptopForYou"), day(0, "10:02")], word("redisDatabase"), ...(step === 0 ? [word("believed"), "ink"] : step === 1 ? [word("contested"), "bad"] : [word("believedTrust"), "gold"])));
    if (step >= 1) r.push(rowEl(["ci-runner", day(1, "16:40")], word("redisCache"), ...(step === 1 ? [word("contested"), "bad"] : [word("defeatedKept"), ""]), step === 2));
    if (step === 2) r.push(rowEl([word("youConsole"), day(2, "09:15")], word("confirmType", {type: V(6).after}), val(V(6).tier), "gold"));
    rows.replaceChildren(...r);
    cap.replaceChildren(capOf(fig, step));
    bConfirm.disabled = step !== 1;
  }
  function go(to) { stop(); from = step; step = to; k = 0; render(); stop = animate(.7, tt => { k = tt === Infinity ? 1 : clamp(tt / .7, 0, 1); S.redraw(); }, () => { from = step; k = 1; S.redraw(); }); }
  bReplay.addEventListener("click", () => { stop(); clearTimeout(timer); step = from = 0; k = 1; render(); S.redraw(); timer = setTimeout(() => go(1), RM.matches ? 0 : 1000); });
  bConfirm.addEventListener("click", () => go(2));
  render(); S.redraw();
  onceVisible(fig, () => bReplay.click());
};

// ---- P18: the receiver decides trust ----
MOUNT[18] = function (fig) {
  const cv = $("canvas", fig), rows = $(".rows", fig), cap = $(".cap", fig);
  const bA = $('[data-a="claim"]', fig), bB = $('[data-a="verdict"]', fig);
  let mode = "claim", k = 1, stop = () => {};
  const S = surface(cv, w => clamp(w * .4, 190, 250), draw);
  function draw(s) {
    const {ctx, W, H} = s, r = clamp(W * .028, 10, 14), L = [W * .17, H * .5], R = [W * .83, H * .5];
    label(ctx, word("desk2Sent"), L[0], H * .13, {align: "center", size: 11.5, color: C.muted, halo: false});
    label(ctx, word("nodeKeeps"), R[0], H * .13, {align: "center", size: 11.5, color: C.muted, halo: false});
    const a = [L[0] + r + 12, L[1]], b = [R[0] - r - 12, R[1]];
    line(ctx, a, b, {alpha: .5});
    if (k < 1) dot(ctx, a[0] + (b[0] - a[0]) * ease(k / .7), a[1], 4, C.ink);
    label(ctx, mode === "claim" ? `trust: "${V(18).claimed}"` : word("entryVerdict"), W / 2, L[1] - 15, {align: "center", mono: true, size: 11});
    dot(ctx, L[0], L[1], r, TYPE.App, {ring: "gold"});
    const rk = ease((k - .65) / .35);
    dot(ctx, R[0], R[1], r, TYPE.App, {ring: mode === "claim" ? "ink" : "gold", ringP: rk});
    label(ctx, "billing-api", L[0], L[1] + r + 15, {align: "center", size: 11.5});
    label(ctx, "billing-api", R[0], R[1] + r + 15, {align: "center", size: 11.5});
    label(ctx, val(mode === "claim" ? V(18).evaluated : V(18).verdict_evaluated), R[0], R[1] + r + 32, {align: "center", mono: true, size: 11, color: mode === "claim" ? C.ink : C.goldHi, alpha: rk});
  }
  function render() {
    rows.replaceChildren(...(mode === "claim"
      ? [rowEl([word("claimed")], `trust: "${V(18).claimed}"`, word("storedAsSent"), ""), rowEl([word("evaluated")], word("signedNoVerdict"), val(V(18).evaluated), "ink")]
      : [rowEl([word("claimed")], `trust: ${V(18).verdict_claimed}`, word("storedAsSent"), ""), rowEl([word("verdict")], word("personMerged"), word("console"), ""), rowEl([word("evaluated")], word("consoleReplayed"), val(V(18).verdict_evaluated), "gold")]));
    cap.replaceChildren(capOf(fig, mode));
    [[bA, "claim"], [bB, "verdict"]].forEach(([b, m]) => { b.classList.toggle("on", mode === m); b.setAttribute("aria-pressed", String(mode === m)); });
  }
  function set(m) { stop(); mode = m; k = 0; render(); stop = animate(1.1, tt => { k = tt === Infinity ? 1 : clamp(tt / 1.1, 0, 1); S.redraw(); }, () => { k = 1; S.redraw(); }); }
  bA.addEventListener("click", () => set("claim"));
  bB.addEventListener("click", () => set("verdict"));
  render(); S.redraw();
  onceVisible(fig, () => set("claim"));
};

// ---- P23: proposing changes nothing ----
MOUNT[23] = function (fig) {
  const cv = $("canvas", fig), cap = $(".cap", fig), B = a => $(`[data-a="${a}"]`, fig);
  const N = {api: [.14, .42], pg: [.46, .2], pgs: [.6, .74], orders: [.88, .46]};
  const BASE = {proposed: false, note: false, verdict: null};
  let st = {proposed: true, note: false, verdict: null}, prev = st, k = 1, stop = () => {}, timer = 0;
  const S = surface(cv, w => clamp(w * .5, 220, 300), draw);
  function draw(s) {
    const {ctx, W, H} = s, r = clamp(W * .025, 9, 13), P = id => [N[id][0] * W, N[id][1] * H], kk = prev === st ? 1 : ease(k);
    const m = st.verdict === "merge" ? (prev.verdict === "merge" ? 1 : kk) : 0;
    const open = s2 => s2.proposed && !s2.verdict;
    const pr = open(st) ? (open(prev) ? 1 : kk) : open(prev) ? 1 - kk : 0;
    const a = P("api"), g = P("pg"), q = P("pgs"), o = P("orders"), gm = [g[0] + (q[0] - g[0]) * m, g[1] + (q[1] - g[1]) * m];
    line(ctx, o, q);
    line(ctx, a, gm, {alpha: EDGE_ALPHA * (1 - pr)});
    if (pr > .01) { line(ctx, a, gm, {color: C.gold, alpha: .9 * pr, w: 1.2}); arrow(ctx, g, q, r, r, pr); }
    dot(ctx, a[0], a[1], r, TYPE.App); label(ctx, "billing-api", a[0], a[1] + r + 13, {align: "center", size: 11.5});
    dot(ctx, o[0], o[1], r, TYPE.App); label(ctx, "orders", o[0], o[1] + r + 13, {align: "center", size: 11.5});
    const nk = st.note && !prev.note ? 1 - kk : 0;
    if (nk > 0) { ctx.globalAlpha = nk; ctx.beginPath(); ctx.arc(o[0], o[1], r + 6 + 10 * (1 - nk), 0, 7); ctx.lineWidth = 2; ctx.strokeStyle = C.teal; ctx.stroke(); ctx.globalAlpha = 1; }
    dot(ctx, q[0], q[1], r, TYPE.Database); label(ctx, "Postgres", q[0] + r + 8, q[1], {size: 12, weight: 600, color: C.ink});
    if (m > .5) label(ctx, word("pgForwards"), q[0], q[1] + r + 14, {align: "center", size: 11, mono: true, color: C.muted, alpha: (m - .5) * 2});
    if (m < 1) { dot(ctx, gm[0], gm[1], r * (1 - .3 * m), TYPE.Database, {alpha: 1 - m}); label(ctx, "pg", gm[0] + r + 8, gm[1], {size: 12, weight: 600, color: C.ink, alpha: 1 - m}); }
    if (pr > .5) label(ctx, word("sameThing"), (g[0] + q[0]) / 2 + 14, (g[1] + q[1]) / 2, {size: 11.5, alpha: pr});
  }
  function sync() {
    B("propose").disabled = st.proposed;
    B("note").disabled = !st.proposed || !!st.verdict || st.note;
    B("merge").disabled = B("reject").disabled = !st.proposed || !!st.verdict;
    const L = V(23).log || {}, K = V(23).canon || {};
    $('[data-k="log"]', fig).textContent = (L.base || 0) + (st.proposed ? L.propose || 0 : 0) + (st.note ? L.note || 0 : 0) + (st.verdict ? L.verdict || 0 : 0);
    $('[data-k="canon"]', fig).textContent = st.verdict ? K[st.verdict] || 0 : K.base || 0;
    cap.replaceChildren(capOf(fig, st.verdict || (st.note ? "note" : st.proposed ? "proposed" : "base")));
  }
  function go(next) { stop(); prev = st; st = next; k = 0; sync(); stop = animate(.8, tt => { k = tt === Infinity ? 1 : clamp(tt / .8, 0, 1); S.redraw(); }, () => { prev = st; k = 1; S.redraw(); }); }
  B("propose").addEventListener("click", () => go({...st, proposed: true}));
  B("note").addEventListener("click", () => go({...st, note: true}));
  B("merge").addEventListener("click", () => go({...st, verdict: "merge"}));
  B("reject").addEventListener("click", () => go({...st, verdict: "reject"}));
  B("reset").addEventListener("click", () => { stop(); clearTimeout(timer); st = prev = BASE; k = 1; sync(); S.redraw(); });
  prev = st; sync(); S.redraw();
  onceVisible(fig, () => { st = prev = BASE; sync(); S.redraw(); timer = setTimeout(() => go({...BASE, proposed: true}), RM.matches ? 0 : 800); });
};

// ---- P5: the answer alphabet ----
MOUNT[5] = function (fig) {
  const qs = $(".qs", fig), ans = $(".ans", fig);
  const btns = $$("button", qs), answers = $$(".p-answers li", ans), kinds = answers.map(li => $(".kind", li).textContent);
  let sel = Math.max(0, btns.findIndex(b => b.getAttribute("aria-pressed") === "true")), k = 1, stop = () => {};
  const OUT = btns.map(b => b.dataset.out), ROUTE = btns.map(b => b.dataset.route || null);
  const cv = $("canvas", fig);
  const S = surface(cv, w => clamp(w * .2, 96, 118), s => {
    const ctx = s.ctx, W = s.W, H = s.H, y = H * .44, you = [20, y], node = [W * .42, y], far = [W - 34, y];
    const out = OUT[sel], remote = ROUTE[sel], off = out === "unavail", gate = out === "withheld";
    line(ctx, you, node, {alpha: .5});
    if (remote) {
      if (off) { line(ctx, node, [mix(node[0], far[0], .45), y], {alpha: .5}); line(ctx, [mix(node[0], far[0], .58), y], far, {alpha: .22}); }
      else line(ctx, node, far, {alpha: .5});
    }
    dot(ctx, you[0], you[1], 6, C.ink2); label(ctx, word("you"), you[0], y + 19, {align: "center", size: 11, halo: false});
    dot(ctx, node[0], node[1], 9, TYPE.App); label(ctx, word("thisNode"), node[0], y + 21, {align: "center", size: 11, halo: false});
    if (remote) {
      if (off) { ctx.globalAlpha = .6; ctx.beginPath(); ctx.arc(far[0], far[1], 9, 0, 7); ctx.strokeStyle = C.muted; ctx.lineWidth = 1.4; ctx.stroke(); ctx.globalAlpha = 1; }
      else dot(ctx, far[0], far[1], 9, C.ink2);
      label(ctx, remote, far[0], y + 21, {align: "center", size: 11, halo: false});
    }
    if (gate) line(ctx, [far[0] - 24, y - 11], [far[0] - 24, y + 11], {color: C.bad, alpha: .9, w: 2.5});
    const stopAt = !remote ? node : off ? [mix(node[0], far[0], .45), y] : gate ? [far[0] - 28, y] : far;
    const path = remote ? [you, node, stopAt, node, you] : [you, node, you];
    const mark = (x, yy) => {
      if (out === "found") dot(ctx, x, yy, 4.5, C.ink);
      else if (out === "notfound") { ctx.beginPath(); ctx.arc(x, yy, 4.5, 0, 7); ctx.strokeStyle = C.ink2; ctx.lineWidth = 1.5; ctx.stroke(); }
      else if (out === "unavail") { ctx.beginPath(); ctx.arc(x, yy, 4.5, 0, 7); ctx.strokeStyle = C.muted; ctx.lineWidth = 1; ctx.stroke(); line(ctx, [x - 3, yy], [x + 3, yy], {color: C.muted, alpha: 1, w: 1.2}); }
      else if (out === "withheld") dot(ctx, x, yy, 4.5, C.bad);
      else for (let i = 0; i < 3; i++) tick(ctx, x, yy - 5 + i * 5, 10, 3, C.ink, 1);
    };
    if (k < 1) { const p = travel(path, k); if (k < .5) dot(ctx, p[0], p[1], 3.5, C.ink); else mark(p[0], p[1]); }
    else { mark(you[0] + 22, y - 17); label(ctx, kinds[sel].toLowerCase(), you[0] + 32, y - 17, {mono: true, size: 10, color: C.ink2, halo: false}); }
  });
  function trip() { stop(); k = 0; stop = animate(1.5, tt => { k = tt === Infinity ? 1 : clamp(tt / 1.5, 0, 1); S.redraw(); }); }
  function show() { answers.forEach((li, i) => li.classList.toggle("on", i === sel)); }
  btns.forEach((b, i) => { if (!b.disabled) b.addEventListener("click", () => { sel = i; btns.forEach((x, j) => x.setAttribute("aria-pressed", String(j === sel))); show(); trip(); }); });
  show(); S.redraw();
  onceVisible(fig, trip);
};

// ---- the scene engine: each principle's animation is a pure function of time, told in beats ----
const PERSON = stableColors(["Person"]).Person;
const lin = (t, a, b) => clamp((t - a) / (b - a), 0, 1);
const win = (t, a, b) => ease((t - a) / (b - a));
const mix = (a, b, k) => a + (b - a) * k;
const hexRgb = h => [1, 3, 5].map(i => parseInt(h.slice(i, i + 2), 16)).join(",");
function well(ctx, x, y, w, h) { ctx.fillStyle = C.well; rrect(ctx, x, y, w, h, Math.min(7, h / 2, w / 2)); ctx.fill(); }
function tick(ctx, x, y, w, h, col, a) { if (a <= 0) return; ctx.globalAlpha = Math.min(1, a); ctx.fillStyle = col; rrect(ctx, x - w / 2, y - h / 2, w, h, Math.min(2.5, w / 2, h / 2)); ctx.fill(); ctx.globalAlpha = 1; }
function glow(ctx, x, y, r, col, a) {
  if (a <= .01) return;
  const g = ctx.createRadialGradient(x, y, 0, x, y, r), c = hexRgb(col);
  g.addColorStop(0, `rgba(${c},${.32 * a})`); g.addColorStop(1, `rgba(${c},0)`);
  ctx.fillStyle = g; ctx.beginPath(); ctx.arc(x, y, r, 0, 7); ctx.fill();
}
function chipW(ctx, s, o) { o = o || {}; ctx.font = `${o.weight || 500} ${o.size || 10.5}px ${o.sans ? SANS : MONO}`; return ctx.measureText(s).width + 14; }
function chip(ctx, s, x, y, o) {
  // a short label on a small ground: field names, tags, tool names, requests. Returns its width.
  o = o || {};
  const size = o.size || 10.5, w = chipW(ctx, s, o), h = size + 10, a = o.alpha == null ? 1 : o.alpha;
  if (a <= .01) return w;
  const x0 = o.align === "center" ? x - w / 2 : o.align === "right" ? x - w : x;
  ctx.globalAlpha = Math.min(1, a);
  ctx.fillStyle = o.bg || C.panel3; rrect(ctx, x0, y - h / 2, w, h, h / 2); ctx.fill();
  if (o.edge) { ctx.strokeStyle = o.edge; ctx.lineWidth = 1; ctx.stroke(); }
  ctx.fillStyle = o.color || C.ink2; ctx.textAlign = "left"; ctx.textBaseline = "middle"; ctx.fillText(s, x0 + 7, y + .5);
  ctx.globalAlpha = 1;
  return w;
}
// The page's owed style (Section 11a): a hatched ground at lowered opacity, a mark no viewer mark uses,
// so an owed demand is never read as the lowest trust or as a superseded line.
function owedBox(ctx, x, y, w, h, a) {
  if (a <= .01 || w <= 0 || h <= 0) return;
  ctx.save(); ctx.globalAlpha = a * .85; ctx.fillStyle = C.well; rrect(ctx, x, y, w, h, Math.min(8, h / 2)); ctx.fill(); ctx.clip();
  ctx.strokeStyle = C.panel3; ctx.lineWidth = 3;
  for (let i = -h; i < w; i += 9) { ctx.beginPath(); ctx.moveTo(x + i, y + h); ctx.lineTo(x + i + h, y); ctx.stroke(); }
  ctx.restore();
}
function owedChip(ctx, x, y, a, align) { return chip(ctx, word("owed"), x, y, {bg: C.well, color: C.ink2, alpha: a, align: align || "right"}); }
function field(ctx, k, v, x, y, o) {
  o = o || {};
  const a = o.alpha == null ? 1 : o.alpha, kw = chipW(ctx, k, {size: 10}) - 14, vw = chipW(ctx, v, {sans: true, size: 11.5}) - 14, w = kw + vw + 24, h = 22;
  ctx.globalAlpha = a; ctx.fillStyle = C.well; rrect(ctx, x, y - h / 2, w, h, h / 2); ctx.fill(); ctx.globalAlpha = 1;
  label(ctx, k, x + 9, y + .5, {mono: true, size: 10, color: C.muted, halo: false, alpha: a});
  label(ctx, v, x + 15 + kw, y + .5, {size: 11.5, color: o.dim ? C.muted : C.ink, halo: false, alpha: a});
  return w;
}
function arrowN(ctx, a, b, ra, rb, al, col, w) {
  const dx = b[0] - a[0], dy = b[1] - a[1], d = Math.hypot(dx, dy) || 1, ux = dx / d, uy = dy / d;
  const sx = a[0] + ux * ra, sy = a[1] + uy * ra, tx = b[0] - ux * rb, ty = b[1] - uy * rb, hl = 7, hw = 3.6;
  ctx.globalAlpha = al; ctx.strokeStyle = col; ctx.fillStyle = col; ctx.lineWidth = w || 1.3;
  ctx.beginPath(); ctx.moveTo(sx, sy); ctx.lineTo(tx - ux * hl * .6, ty - uy * hl * .6); ctx.stroke();
  ctx.beginPath(); ctx.moveTo(tx, ty); ctx.lineTo(tx - ux * hl - uy * hw, ty - uy * hl + ux * hw); ctx.lineTo(tx - ux * hl + uy * hw, ty - uy * hl - ux * hw); ctx.closePath(); ctx.fill();
  ctx.globalAlpha = 1;
}
function ring(ctx, x, y, r, kind, a) {
  // the viewer's trust rings, told by weight: the lowest tier 1 muted, a host's signature 1.6 ink, a person's confirmation 2.4 gold
  if (a <= 0) return;
  ctx.globalAlpha = a; ctx.beginPath(); ctx.arc(x, y, r + 3.5, 0, 7);
  if (kind === "lowest") { ctx.lineWidth = 1; ctx.strokeStyle = C.muted; }
  else if (kind === "ink") { ctx.lineWidth = 1.6; ctx.strokeStyle = C.ink; }
  else { ctx.lineWidth = 2.4; ctx.strokeStyle = C.goldHi; }
  ctx.stroke(); ctx.globalAlpha = 1;
}
function cloud(ctx, pts, pad, a) {
  // what was said together: a soft field under the members (the stage's region)
  if (a <= .01 || !pts.length) return;
  const cx = pts.reduce((s, p) => s + p[0], 0) / pts.length, cy = pts.reduce((s, p) => s + p[1], 0) / pts.length;
  const rx = Math.max(...pts.map(p => Math.abs(p[0] - cx))) + pad, ry = Math.max(...pts.map(p => Math.abs(p[1] - cy))) + pad * .8;
  ctx.save(); ctx.translate(cx, cy); ctx.scale(1, ry / rx);
  const g = ctx.createRadialGradient(0, 0, 0, 0, 0, rx);
  const ink = hexRgb(C.ink);
  g.addColorStop(0, `rgba(${ink},${.11 * a})`); g.addColorStop(.75, `rgba(${ink},${.06 * a})`); g.addColorStop(1, `rgba(${ink},0)`);
  ctx.fillStyle = g; ctx.beginPath(); ctx.arc(0, 0, rx, 0, 7); ctx.fill(); ctx.restore();
}
function wrapLines(ctx, s, maxW, font) {
  ctx.font = font; const out = []; let cur = "";
  for (const w of s.split(" ")) { const nx = cur ? cur + " " + w : w; if (ctx.measureText(nx).width > maxW && cur) { out.push(cur); cur = w; } else cur = nx; }
  if (cur) out.push(cur); return out;
}
function travel(path, k) {
  // a point k of the way along a polyline
  const seg = []; let L = 0;
  for (let i = 1; i < path.length; i++) { const d = Math.hypot(path[i][0] - path[i - 1][0], path[i][1] - path[i - 1][1]); seg.push(d); L += d; }
  let s = clamp(k, 0, 1) * L;
  for (let i = 0; i < seg.length; i++) { if (s <= seg[i] || i === seg.length - 1) { const u = seg[i] ? s / seg[i] : 0; return [mix(path[i][0], path[i + 1][0], u), mix(path[i][1], path[i + 1][1], u)]; } s -= seg[i]; }
  return path[path.length - 1];
}
// a made-up project, the one the landing's stage uses
const G_BILL = {
  ent: [
    {n: [["api", "App"]], e: []},
    {n: [["pg", "Database"]], e: []},
    {n: [["api", null], ["pg", null]], e: [["api", "pg"]]},
    {n: [["wh", "Feature"]], e: []},
    {n: [["api", null], ["wh", null]], e: [["api", "wh"]]},
    {n: [["wh", null], ["idem", "Pattern"]], e: [["wh", "idem"]]},
  ],
  pos: {api: [.2, .3], pg: [.8, .18], wh: [.66, .66], idem: [.26, .86]},
  name: {api: "billing-api", pg: "Postgres", wh: "webhooks", idem: "idempotency"},
};
// the engine's fingerprint of the six entries (the cases, U12)
const G_FP = FP;
function drawGraph(ctx, G, born, t, rect, ga, o) {
  o = o || {};
  if (ga <= 0) return null;
  const [gx, gy, gw, gh] = rect, nodes = {}, edges = [];
  G.ent.forEach((e, k) => {
    const b = born(k); if (b == null || t < b) return;
    e.n.forEach(([id, kind]) => { const n = nodes[id] || (nodes[id] = {born: b, kind: null, kindAt: 0}); n.born = Math.min(n.born, b); if (kind) { n.kind = kind; n.kindAt = b; } });
    e.e.forEach(([a, c]) => edges.push([a, c, b]));
  });
  const P = id => [gx + G.pos[id][0] * gw, gy + G.pos[id][1] * gh], r = o.r || clamp(Math.min(gw, gh * 1.6) * .045, 5, 10);
  edges.forEach(([a, c, b]) => line(ctx, P(a), P(c), {p: ease((t - b) / .45), alpha: EDGE_ALPHA * ga}));
  for (const id in nodes) {
    const n = nodes[id], sc = ease((t - n.born) / .35), [x, y] = P(id), kk = n.kind ? ease((t - n.kindAt) / .4) : 0;
    if (kk < 1) dot(ctx, x, y, r * sc, CONCEPT, {alpha: ga});
    if (kk > 0) dot(ctx, x, y, r * sc, TYPE[n.kind], {alpha: kk * ga});
    if (o.labels !== false && gw > 140 && sc > .3) { const left = G.pos[id][0] > .6; label(ctx, G.name[id], left ? x - r - 6 : x + r + 6, y, {align: left ? "right" : "left", size: 11.5, alpha: sc * ga}); }
  }
  return P;
}
const SCENE = {};
// A scene's beats are the page's list: each item names its end time, its word and its caption. The
// list becomes a row of buttons and one caption beneath them, announced as each beat begins.
function mountScene(p, fig) {
  const sc = SCENE[p], cv = $("canvas", fig), items = $$(".beats li", fig);
  const B = items.map(li => ({t: +li.dataset.t, k: $(".bk", li).textContent, cap: $(".bc", li)})), END = B[B.length - 1].t;
  const bar = el("div", {class: "beatbar", role: "group", "aria-label": word("steps")}), cap = el("p", {class: "cap", "aria-live": "off"});
  const btns = B.map((b, i) => { const btn = el("button", {type: "button", class: "beat"}); btn.append(el("b", null, String(i + 1)), b.k); bar.append(btn); return btn; });
  const play = el("button", {type: "button", class: "beat play"}, word(PHONE.matches ? "playScene" : "replay"));
  bar.append(play); $(".beats", fig).after(bar, cap);
  let t = END, shown = -1, stop = () => {}, playing = false;
  const S = surface(cv, sc.h || (w => clamp(w * .6, 210, 260)), s => sc.draw(s.ctx, s.W, s.H, t));
  const beatAt = tt => { for (let i = 0; i < B.length; i++) if (tt <= B[i].t) return i; return B.length - 1; };
  function show(i) {
    if (i === shown) return; shown = i;
    cap.replaceChildren(cloneKids(B[i].cap));
    btns.forEach((b, j) => b.setAttribute("aria-current", j === i ? "step" : "false"));
  }
  const rest = () => { playing = false; play.textContent = word("replay"); };
  function run(a, b) {
    stop(); playing = true; play.textContent = word("stop");
    stop = animate(b - a, tt => { t = tt === Infinity ? b : a + tt; show(beatAt(t)); S.redraw(); }, rest);
  }
  btns.forEach((btn, i) => btn.addEventListener("click", () => run(i ? B[i - 1].t : 0, B[i].t)));
  // while a scene plays its control reads Stop, which ends it at its resting frame (WCAG 2.2.2)
  play.addEventListener("click", () => { if (playing) { stop(); t = END; show(B.length - 1); S.redraw(); rest(); } else run(0, END); });
  show(B.length - 1); S.redraw();
  if (!PHONE.matches || fig.closest(".pr.open")) onceVisible(fig, () => run(0, END));
}

// ---- P1 ----
SCENE[1] = {
  h: w => clamp(w * .6, 210, 250),
  draw(ctx, W, H, t) {
    const at = k => .3 + k * .42, rr = k => 4.7 + k * .36, replay = t >= 4.5;
    const wx = 14, wy = 14, ww = W - 28, wh = 20, sw = ww / 6;
    well(ctx, wx, wy, ww, wh);
    for (let k = 0; k < 6; k++) {
      if (t < at(k)) continue;
      const f1 = t < 4.5 ? 1 - lin(t, at(k), at(k) + .5) : 0, f2 = t >= rr(k) ? 1 - lin(t, rr(k), rr(k) + .5) : 0, fl = Math.max(f1, f2), x = wx + sw * (k + .5);
      tick(ctx, x, wy + wh / 2, sw * .7, wh - 7, fl > .05 ? C.ink : C.ink2, .62 + .38 * fl);
      label(ctx, String(k + 1), x, wy + wh / 2 + .5, {align: "center", mono: true, size: 10, weight: 600, color: C.bg, halo: false});
    }
    if (replay && t < rr(5) + .5) { const x = wx + sw * clamp((t - rr(0)) / .36 + .5, .5, 5.5); line(ctx, [x, wy - 5], [x, wy + wh + 5], {color: C.gold, alpha: .9, w: 2}); }
    const ga = replay ? 1 : 1 - lin(t, 3.2, 3.9), gy = wy + wh + 18;
    drawGraph(ctx, G_BILL, k => replay ? rr(k) : at(k), t, [22, gy, W - 44, H - gy - 30], ga);
    const gone = !replay && ga < .05;
    label(ctx, gone ? word("noGraph") : word("graphFp", {fp: G_FP}), 14, H - 14, {mono: true, size: 11, color: gone ? C.muted : C.ink2, halo: false});
    if (replay && t > rr(5) + .3) label(ctx, word("sameAsBefore"), W - 14, H - 14, {align: "right", size: 11.5, color: C.ink, halo: false, alpha: lin(t, rr(5) + .3, rr(5) + .8)});
  },
};

// ---- P2 ----
SCENE[2] = {
  h: w => w < 420 ? clamp(w * 1.0, 250, 330) : clamp(w * .6, 230, 270),
  draw(ctx, W, H, t) {
    const narrow = W < 420, lx = 14, lw = 24, ly = 14, lh = H - 28, n = 6, sel = 3;
    well(ctx, lx, ly, lw, lh);
    const ty = i => ly + 14 + i * (lh - 28) / (n - 1), E = [lx + lw / 2, ty(sel)], hit = win(t, 4.8, 5.6);
    for (let i = 0; i < n; i++) tick(ctx, E[0], ty(i), lw - 10, 6, i === sel ? C.ink : C.ink2, i === sel ? 1 : .4);
    glow(ctx, E[0], E[1], 28, C.ink, Math.max(1 - lin(t, .2, 1.2), hit * (1 - lin(t, 5.8, 6.6))));
    const F = [[word("fHost"), "laptop"], [word("fFor"), word("you")], [word("fWorkspace"), "billing"], [word("fObserved"), day(0, "10:02")], [word("fSource"), "git:3fa91c2"], [word("fConfidence"), val(V(2).confidence)]];
    const x1 = lx + lw + 34, pos = i => narrow ? [x1, H * .3 + i * (H * .62 / 5)] : [i < 3 ? x1 : W * .56, H * [.42, .64, .86][i % 3]];
    let cw = 0;
    F.forEach(([k, v], i) => {
      const a = win(t, .3 + i * .36, .7 + i * .36); if (a <= 0) return;
      const [x, y] = pos(i);
      line(ctx, E, [x - 3, y], {alpha: .2 * a});
      const w = field(ctx, k, v, x, y, {alpha: a, dim: i === 5}); if (i === 5) cw = w;
    });
    const g = lin(t, 3.1, 4.4);
    if (g > 0 && g < 1) {
      const [x, y] = pos(5), gx = x + cw - 26, gy = y - 24 + 10 * g, al = Math.sin(g * Math.PI);
      label(ctx, "0.9?", gx, gy, {mono: true, size: 11.5, color: C.bad, alpha: al, halo: false});
      if (g > .45) line(ctx, [gx - 3, gy], [gx + 32, gy], {color: C.bad, alpha: al, w: 1.4});
    }
    if (hit > 0) {
      const hy = narrow ? H * .1 : H * .16;
      chip(ctx, word("hit"), x1, hy, {sans: true, size: 11, color: C.ink, alpha: hit});
      ctx.globalAlpha = hit; ctx.strokeStyle = C.ink; ctx.lineWidth = 1.3; ctx.beginPath(); ctx.moveTo(x1 - 3, hy); ctx.quadraticCurveTo(E[0] + 26, hy, E[0] + lw / 2 + 2, E[1]); ctx.stroke(); ctx.globalAlpha = 1;
    }
  },
};

// ---- P3 ----
SCENE[3] = {
  h: w => clamp(w * .62, 220, 260),
  draw(ctx, W, H, t) {
    const N = [W * .19, H * .46], r = 11, v2 = win(t, 1.9, 2.6), first = win(t, .3, .8);
    dot(ctx, N[0], N[1], r, TYPE.App);
    label(ctx, "billing-api", N[0], N[1] - r - 13, {align: "center", size: 12, weight: 600, color: C.ink});
    label(ctx, "Postgres 14", N[0], N[1] + r + 15, {align: "center", size: 11.5, alpha: first * (1 - v2)});
    label(ctx, V(3).shown, N[0], N[1] + r + 15, {align: "center", size: 11.5, color: C.ink, alpha: v2});
    const lx = W * .4, lw = W - lx - 12, rowY = i => H - 42 - i * 46;
    well(ctx, lx, 12, lw, H - 24);
    const count = t < .3 ? 0 : t < 1.9 ? 1 : V(3).entries;
    label(ctx, word(PLURAL.select(count) === "one" ? "entryInLog" : "entriesInLog", {n: count}), lx + 10, 28, {mono: true, size: 10.5, color: C.muted, halo: false});
    const row = (i, a, text, tag, fl) => {
      if (a <= 0) return; const y = rowY(i), x = lx + 16;
      tick(ctx, x, y, 7, 14, fl > .05 ? C.ink : C.ink2, a);
      label(ctx, text, x + 12, y, {size: 11.5, color: C.ink, alpha: a, halo: false});
      if (tag) label(ctx, tag, x + 12, y + 15, {mono: true, size: 10, color: C.muted, alpha: a, halo: false});
    };
    row(0, first * (1 - .45 * v2), "Postgres 14", v2 > .5 ? word("superseded") : "", 1 - lin(t, .3, .9));
    row(1, win(t, 1.9, 2.4), V(3).shown, t > 4.6 ? word("keptOnce") : "", Math.max(t >= 1.9 ? 1 - lin(t, 1.9, 2.5) : 0, t > 4.5 ? 1 - lin(t, 4.5, 5.1) : 0));
    const g = lin(t, 3.8, 4.6);
    if (g > 0 && t < 4.65) { const y = rowY(1), x = mix(lx + lw + 20, lx + 16, ease(g)); tick(ctx, x, y, 7, 14, C.ink, 1); label(ctx, V(3).shown, x + 12, y, {size: 11.5, color: C.ink2, halo: false, alpha: 1 - g * .5}); }
    const d = win(t, 5.5, 6.1);
    if (d > 0) {
      const y = 60; owedBox(ctx, lx + 8, y - 13, lw - 16, 26, d);
      label(ctx, word("destroy"), lx + 18, y, {size: 11.5, color: C.muted, alpha: d, halo: false}); owedChip(ctx, lx + lw - 14, y, d);
    }
  },
};

// ---- P4 ----
SCENE[4] = {
  h: w => clamp(w * .64, 230, 270),
  draw(ctx, W, H, t) {
    const x0 = 18, x1 = W - 18, m = i => x0 + (x1 - x0) * i / 6, yW = H * .38, yS = H * .7;
    // asking as of a date is owed: its band lies under everything else
    const g4 = win(t, 5.1, 6.4), a4 = Math.min(1, lin(t, 5.1, 5.5)), xs = mix(m(.6), m(1.15), g4);
    owedBox(ctx, xs - 12, 26, 24, yS - 20, a4 * .9);
    [[yW, word("trueWorld")], [yS, word("knownSystem")]].forEach(([y, s]) => {
      line(ctx, [x0, y], [x1, y], {alpha: .5});
      for (let i = 0; i <= 6; i++) line(ctx, [m(i), y - 3], [m(i), y + 3], {alpha: .5});
      label(ctx, s, x0, y - 30, {size: 11, color: C.muted, halo: false});
    });
    [0, 1, 2, 3, 4, 5].forEach(i => label(ctx, month(i), m(i + .5), yS + 15, {align: "center", mono: true, size: 9.5, color: C.muted, halo: false}));
    const xa = m(.3), g1 = win(t, .3, 1.8);
    if (g1 > 0) { ctx.fillStyle = TYPE.Database; rrect(ctx, xa, yW - 3.5, (x1 - xa) * g1, 7, 3.5); ctx.fill(); label(ctx, word("usesPgSince", {date: date(0, 10)}), xa, yW - 14, {size: 11.5, color: C.ink, alpha: g1, halo: false}); }
    const xl = m(2.07), g2 = win(t, 2.1, 2.8), g2b = win(t, 2.8, 3.4);
    if (g2 > 0) {
      line(ctx, [xl, yS], [xl, yW + 5], {alpha: .45 * g2, color: C.ink2, w: 1});
      dot(ctx, xl, yS, 5, C.ink, {alpha: g2});
      label(ctx, word("learnedOn", {date: date(2, 3)}), xl + 9, yS - 12, {size: 11.5, color: C.ink, alpha: g2, halo: false});
    }
    if (g2b > 0) { ctx.globalAlpha = .6 * g2b; ctx.fillStyle = C.muted; rrect(ctx, xa, yW + 7, xl - xa, 3, 1.5); ctx.fill(); ctx.globalAlpha = 1; label(ctx, word("trueNotKnown"), xa, yW + 18, {mono: true, size: 10, color: C.muted, alpha: g2b, halo: false}); }
    const g3 = win(t, 3.9, 4.5);
    if (g3 > 0) { const y = H - 15, w1 = chip(ctx, word("validOn", {date: date(0, 10)}), x0, y, {alpha: g3, color: C.ink}); chip(ctx, word("recordedOn", {date: date(2, 3)}), x0 + w1 + 8, y, {alpha: win(t, 4.1, 4.7), color: C.ink}); }
    if (a4 > 0) { const x = xs; line(ctx, [x, 26], [x, yS + 6], {color: C.muted, alpha: .7 * a4, w: 1}); label(ctx, word("asOf", {date: date(1, 1)}), x + 6, 15, {size: 11, color: C.ink2, alpha: a4, halo: false}); owedChip(ctx, W - 14, 15, a4); }
  },
};
// ---- P7 ----
SCENE[7] = {
  h: w => clamp(w * .7, 236, 276),
  draw(ctx, W, H, t) {
    const wx = 14, wy = 14, ww = W - 28, wh = 18, n = 10, sw = ww / n, s = lin(t, .4, 2.3) * (n + 1);
    well(ctx, wx, wy, ww, wh);
    for (let k = 0; k < n; k++) { const hl = clamp(1 - Math.abs(s - k - .5) / 1.6, 0, 1); tick(ctx, wx + sw * (k + .5), wy + wh / 2, sw * .5, wh - 7, hl > .3 ? C.ink : C.ink2, .45 + .55 * hl); }
    label(ctx, word("idleReads"), wx, wy + wh + 14, {mono: true, size: 10, color: C.muted, halo: false, alpha: t < 4.4 ? 1 : .55});
    chip(ctx, word("mergePg"), wx, 78, {sans: true, size: 11.5, color: C.ink2, bg: C.bg, edge: C.gold, alpha: win(t, 2.7, 3.2)});
    chip(ctx, word("promoteContext"), wx, 108, {sans: true, size: 11.5, color: C.ink2, bg: C.bg, edge: C.gold, alpha: win(t, 3.1, 3.6)});
    const pulse = t > 3.5 && t < 4.5 ? Math.sin(lin(t, 3.5, 4.5) * Math.PI) : 0;
    if (t > 2.7) label(ctx, word("canonZero"), wx, 136, {mono: true, size: 10.5, color: pulse > .15 ? C.ink : C.muted, halo: false, alpha: win(t, 2.7, 3.1)});
    const a = win(t, 4.5, 4.9);
    if (a > 0) {
      const y0 = 168, dy = 23, r = win(t, 4.8, 6.1), rows = [["Postgres 16", month(9)], ["Postgres 14", month(2)], ["MySQL trial", month(0)]], from = [2, 1, 0];
      owedBox(ctx, wx - 6, y0 - 32, W - wx - 2, H - y0 + 24, a);
      label(ctx, word("retrieval"), wx, y0 - 20, {mono: true, size: 10, color: C.muted, halo: false, alpha: a});
      owedChip(ctx, W - 14, y0 - 20, a);
      [[from, 1 - r, 0], [[0, 1, 2], r, 1]].forEach(([order, k, after]) => {
        if (k <= .01) return;
        rows.forEach(([nm, mo], i) => {
          const y = y0 + order[i] * dy + (after ? 1 - r : -r) * 6, al = a * .62 * k * (after && i === 2 ? .55 : 1);
          tick(ctx, wx + 4, y, 6, 12, C.ink2, al);
          label(ctx, nm, wx + 14, y, {size: 11.5, color: C.ink2, halo: false, alpha: al});
          label(ctx, mo, W - 14, y, {align: "right", mono: true, size: 10, color: C.muted, halo: false, alpha: al});
        });
      });
    }
  },
};

// ---- P8 ----
SCENE[8] = {
  h: w => clamp(w * .66, 230, 270),
  draw(ctx, W, H, t) {
    const vx = W * .6, vw = W - vx - 12, vy = 12, vh = H * .56, door = vx - 6, y = H * .25;
    well(ctx, vx, vy, vw, vh);
    label(ctx, word("vocabulary"), vx + 10, vy + 14, {mono: true, size: 10, color: C.muted, halo: false});
    ["App", "Database", "Feature"].forEach((s, i) => { const yy = vy + 36 + i * 22; dot(ctx, vx + 18, yy, 5, TYPE[s]); label(ctx, s, vx + 30, yy, {size: 11.5, halo: false}); });
    const inC = win(t, 3.6, 4.1);
    if (inC > 0) { const yy = vy + 36 + 66; dot(ctx, vx + 18, yy, 5 * inC, TYPE.Cache); label(ctx, "Cache", vx + 30, yy, {size: 11.5, color: C.ink, alpha: inC, halo: false}); glow(ctx, vx + 18, yy, 22, C.ink, 1 - lin(t, 4.0, 4.6)); }
    line(ctx, [door, vy + 6], [door, vy + vh - 6], {color: C.ink2, alpha: .45, w: 2});
    const cw = chipW(ctx, word("defineCache"), {sans: true, size: 11.5});
    const b1 = lin(t, .3, 2.2);
    if (b1 > 0 && b1 < 1) {
      const go = b1 < .5 ? ease(b1 * 2) : 1 - ease((b1 - .5) * 2) * .7, x = mix(14, door - cw - 4, go);
      chip(ctx, word("defineCache"), x, y, {sans: true, size: 11.5, color: C.ink, alpha: 1 - lin(b1, .8, 1)});
      if (b1 > .45) label(ctx, word("needsDefinition"), 14, y + 25, {size: 11, color: C.bad, halo: false, alpha: Math.sin(lin(b1, .45, 1) * Math.PI)});
    }
    const b2 = lin(t, 2.6, 3.8);
    if (b2 > 0 && b2 < 1) {
      const x = mix(14, door - cw * .5, ease(b2)), al = 1 - lin(b2, .8, 1);
      chip(ctx, word("defineCache"), x, y, {sans: true, size: 11.5, color: C.ink, alpha: al});
      label(ctx, '"a store of copies"', x + 4, y + 22, {size: 10.5, color: C.ink2, halo: false, alpha: al});
    }
    const by = H * .82, nx = 22, b3 = win(t, 4.5, 4.9);
    if (b3 > 0) {
      dot(ctx, nx, by, 8, TYPE.Cache, {alpha: b3});
      label(ctx, "Redis", nx + 15, by - 9, {size: 12, weight: 600, color: C.ink, halo: false, alpha: b3});
      const flash = t > 5.6 ? 1 - lin(t, 5.6, 6.3) : 0;
      label(ctx, '"a cache for sessions"', nx + 15, by + 8, {size: 11, color: flash > .1 ? C.ink : C.ink2, halo: false, alpha: b3});
      const g = lin(t, 4.8, 5.7), bw = chipW(ctx, word("descBlank"), {size: 10});
      if (g > 0 && g < 1) chip(ctx, word("descBlank"), mix(W - 14 - bw, Math.min(nx + 150, W - 14 - bw), ease(g)), by, {size: 10, color: C.muted, alpha: 1 - lin(g, .75, 1)});
      if (t > 5.7) label(ctx, word("kept"), W - 14, by + 8, {align: "right", mono: true, size: 10.5, color: C.muted, halo: false, alpha: win(t, 5.7, 6.1)});
    }
  },
};

// ---- P9 ----
SCENE[9] = {
  h: w => clamp(w * .7, 240, 280),
  draw(ctx, W, H, t) {
    const S0 = [W * .14, H * .25], cx = W * .3, d1 = win(t, .3, .9), d2 = win(t, 1.0, 1.6), bad = win(t, 1.7, 2.2);
    if (d1 > 0) line(ctx, S0, [cx - 4, H * .12], {alpha: .4 * d1});
    if (d2 > 0) line(ctx, S0, [cx - 4, H * .38], {alpha: .4 * d2});
    chip(ctx, "laptop: a running process", cx, H * .12, {sans: true, size: 11, alpha: d1});
    chip(ctx, "desk-2: a support plan", cx, H * .38, {sans: true, size: 11, alpha: d2});
    dot(ctx, S0[0], S0[1], 10, C.ink2, {bad});
    label(ctx, "Service", S0[0], S0[1] + 23, {align: "center", size: 11.5, weight: 600, color: C.ink, halo: false});
    if (bad > .5) label(ctx, word("contested"), S0[0], S0[1] + 38, {align: "center", mono: true, size: 10, color: C.bad, halo: false, alpha: bad});
    const rA = win(t, 2.7, 3.1), ry = H * .6;
    chip(ctx, word("relationType"), 14, ry, {size: 10.5, color: C.ink2, alpha: rA});
    const gate = W * .78, py = H * .78, pw = chipW(ctx, word("addType"), {size: 10.5}), mv = win(t, 3.1, 4.1), blocked = win(t, 4.1, 4.5);
    if (rA > 0) {
      chip(ctx, word("addType"), mix(14, Math.max(14, gate - pw - 8), mv), py, {size: 10.5, color: C.ink, bg: C.bg, edge: C.gold, alpha: rA});
      line(ctx, [gate, py - 14], [gate, py + 14], {color: blocked > .5 ? C.bad : C.ink2, alpha: .5 + .5 * blocked, w: 2.5});
      if (blocked > 0) label(ctx, word("blocked"), gate + 8, py, {size: 11, color: C.bad, halo: false, alpha: blocked});
      if (blocked > 0) line(ctx, [14 + chipW(ctx, word("relationType"), {size: 10.5}) * .5, ry + 10], [mix(14, Math.max(14, gate - pw - 8), 1) + pw * .5, py - 10], {color: C.bad, alpha: .5 * blocked, w: 1});
    }
    const a3 = win(t, 5.1, 5.6);
    if (a3 > 0) { owedBox(ctx, 8, H - 25, W - 16, 26, a3); label(ctx, word("subtypeCycles"), 14, H - 12, {size: 11, color: C.muted, halo: false, alpha: a3}); owedChip(ctx, W - 14, H - 12, a3); }
  },
};

// ---- P10 ----
SCENE[10] = {
  h: w => clamp(w * .7, 240, 280),
  draw(ctx, W, H, t) {
    const top = [12, 12, W - 24, H * .44], bot = [12, H * .44 + 22, W - 24, H * .56 - 34];
    well(ctx, ...top); well(ctx, ...bot);
    label(ctx, word("yourVocabulary"), top[0] + 10, top[1] + 14, {mono: true, size: 10, color: C.muted, halo: false});
    label(ctx, word("coreModel"), bot[0] + 10, bot[1] + 14, {mono: true, size: 10, color: C.muted, halo: false});
    const flow = (items, box, at, o) => {
      let x = box[0] + 10, y = box[1] + 38;
      items.forEach((s, i) => {
        const w = chipW(ctx, s, o); if (x + w > box[0] + box[2] - 8) { x = box[0] + 10; y += 26; }
        const a = at ? win(t, at(i), at(i) + .4) : 1;
        chip(ctx, s, x - (1 - a) * 14, y, {...o, alpha: a, color: o.color(i)}); x += w + 6;
      });
    };
    const dom = ["Project", "Decision", "Tool", "Cache", "Runbook"];
    flow(dom, top, i => .4 + i * .5, {sans: true, size: 11.5, color: () => C.ink});
    const v2 = win(t, 3.6, 4.2), core = ["Observation", "Entity", "Relation", "Provenance", "Host", "Workspace"];
    flow(core, bot, null, {size: 10.5, color: () => v2 > 0 && t < 5 ? C.ink : C.ink2});
    chip(ctx, v2 > .5 ? "v2" : "v1", bot[0] + bot[2] - 10, bot[1] + 14, {align: "right", size: 10, color: v2 > .5 ? C.ink : C.muted});
    if (t > 3.4 && t < 4.6) glow(ctx, W / 2, bot[1] + bot[3] / 2, W * .45, C.ink, Math.sin(lin(t, 3.4, 4.6) * Math.PI) * .6);
    const m = win(t, 4.3, 4.9);
    if (m > 0) { const y = bot[1] + bot[3] - 14; chip(ctx, word("migration"), bot[0] + 10, y, {size: 10, color: C.ink, bg: C.bg, alpha: m}); }
    const fl = Math.max(...dom.map((_, i) => t > .4 + i * .5 && t < 1 + i * .5 ? 1 : 0));
    label(ctx, fl ? word("fromLog") : "", top[0] + top[2] - 10, top[1] + 14, {align: "right", mono: true, size: 10, color: C.muted, halo: false});
  },
};

// ---- P11 ----
SCENE[11] = {
  h: w => clamp(w * .7, 240, 280),
  draw(ctx, W, H, t) {
    const wx = 14, wy = 14, ww = W - 28, wh = 18, at = [.3, .8, 1.4, 2.6, 3.3], sw = ww / 6;
    well(ctx, wx, wy, ww, wh);
    const gy = 48, gh = H - gy - 20, P = (x, y) => [14 + x * (W - 28), gy + y * gh];
    const N = {wh: P(.18, .34), idem: P(.42, .56), retry: P(.2, .8), stripe: P(.8, .3), ledger: P(.82, .74)};
    const NAME = {wh: "webhooks", idem: "idempotency", retry: "retries", stripe: "Stripe", ledger: "ledger"};
    const MENT = [["stripe"], ["ledger", "stripe"], ["wh", "idem", "retry"], ["wh", "idem", "retry"], ["retry", "wh", "idem"]];
    const born = {};
    MENT.forEach((ms, k) => ms.forEach(id => { if (born[id] == null) born[id] = at[k]; }));
    MENT.forEach((ms, k) => {
      if (t < at[k]) return; const x = wx + sw * (k + .5), fl = 1 - lin(t, at[k], at[k] + .6);
      tick(ctx, x, wy + wh / 2, sw * .55, wh - 7, fl > .05 ? C.ink : C.ink2, .6 + .4 * fl);
      if (fl > 0) ms.forEach(id => line(ctx, [x, wy + wh], N[id], {alpha: .5 * fl, color: C.ink2}));
    });
    const co = [2, 3, 4].filter(k => t >= at[k]).length, cg = co ? win(t, at[2], at[2] + .6) * (.45 + .2 * co) : 0;
    cloud(ctx, [N.wh, N.idem, N.retry], 30, cg);
    if (t >= at[1]) line(ctx, N.ledger, N.stripe, {p: ease((t - at[1]) / .45)});
    const pr = win(t, 4.6, 5.3), D = P(W < 340 ? .44 : .6, .1);
    if (pr > 0) [N.wh, N.idem, N.retry].forEach(q => line(ctx, D, q, {color: C.muted, alpha: .55 * pr, w: 1}));
    for (const id in N) {
      if (t < born[id]) continue; const sc = ease((t - born[id]) / .35), [x, y] = N[id];
      dot(ctx, x, y, 7 * sc, CONCEPT);
      label(ctx, NAME[id], x + 11, y, {size: 11.5, alpha: sc});
    }
    if (t > at[2] + .3 && t < 4.6) label(ctx, word("saidTogether", {n: co}), N.idem[0] + 11, N.idem[1] + 16, {mono: true, size: 10, color: C.muted, halo: false});
    if (pr > 0) {
      dot(ctx, D[0], D[1], 8 * pr, C.ink2); ring(ctx, D[0], D[1], 8, "lowest", pr);
      label(ctx, "webhook delivery", D[0] + 15, D[1], {size: 11.5, color: C.ink, alpha: pr});
      label(ctx, word("derivedLowest"), D[0] + 15, D[1] + 15, {mono: true, size: 10, color: C.muted, halo: false, alpha: pr});
    }
    const a4 = win(t, 5.7, 6.2);
    if (a4 > 0) { const tw = chipW(ctx, word("typeCandidate"), {size: 10}) + 54; owedBox(ctx, W - 14 - tw - 6, H - 27, tw + 6, 26, a4); label(ctx, word("typeCandidate"), W - 20, H - 14, {align: "right", mono: true, size: 10, color: C.muted, halo: false, alpha: a4}); owedChip(ctx, W - 20 - chipW(ctx, word("typeCandidate"), {size: 10}) - 6, H - 14, a4); }
  },
};

// ---- P12 ----
SCENE[12] = {
  h: w => clamp(w * .66, 230, 270),
  draw(ctx, W, H, t) {
    const g = win(t, .2, .9);
    drawGraph(ctx, G_BILL, () => 0, 9, [22, 14, W - 44, H * .44], g);
    label(ctx, word("graphFp", {fp: G_FP}), W - 14, H * .52, {align: "right", mono: true, size: 10.5, color: t > 3.4 && t < 4.4 ? C.ink : C.muted, halo: false});
    const by = H * .6;
    line(ctx, [14, by], [W - 14, by], {alpha: .4, color: C.ink2, w: 1});
    label(ctx, word("port"), 14, by - 10, {mono: true, size: 10, color: C.muted, halo: false});
    const sw = Math.min(170, W * .5), sx = (W - sw) / 2, sy = H * .7, sh = H * .2, out = win(t, 2.0, 2.9), inn = win(t, 2.7, 3.6);
    ctx.save(); ctx.beginPath(); ctx.rect(0, by + 4, W, H - by); ctx.clip();
    [[out, "cozo", 1], [inn, "redb", 0]].forEach(([k, s, leaving]) => {
      const off = leaving ? k * (sh + 30) : (1 - k) * (sh + 30), a = leaving ? 1 - k : k;
      if (a <= 0) return; ctx.globalAlpha = a; well(ctx, sx, sy + off, sw, sh); ctx.globalAlpha = 1;
      label(ctx, word("storeIs", {s}), sx + sw / 2, sy + off + sh / 2, {align: "center", mono: true, size: 11.5, color: C.ink, halo: false, alpha: a});
    });
    ctx.restore();
    const c = lin(t, 4.4, 5.4), cw = chipW(ctx, word("btreePage"), {size: 10});
    if (c > 0) {
      const y = mix(sy - 4, by + 16, ease(Math.min(1, c * 1.4)));
      chip(ctx, word("btreePage"), sx + sw + 10 > W - cw - 10 ? sx : sx + sw + 10, y, {size: 10, color: C.ink2, alpha: 1 - lin(t, 5.6, 6.0) * .4});
      if (c > .7) { const x = (sx + sw + 10 > W - cw - 10 ? sx : sx + sw + 10) + cw / 2, a = win(t, 5.0, 5.3); line(ctx, [x - 5, by - 5], [x + 5, by + 5], {color: C.bad, alpha: a, w: 2}); line(ctx, [x + 5, by - 5], [x - 5, by + 5], {color: C.bad, alpha: a, w: 2}); }
    }
  },
};

// ---- P13 ----
SCENE[13] = {
  h: w => clamp(w * .62, 220, 260),
  draw(ctx, W, H, t) {
    owedBox(ctx, W * .36, 30, W * .64 - 8, H - 38, 1);
    owedChip(ctx, W - 14, 16, 1);
    const tx = W * .12, a0 = .5;
    dot(ctx, tx, H * .3, 6, C.muted, {alpha: a0}); label(ctx, "Person", tx + 10, H * .3, {size: 11, color: C.muted, halo: false, alpha: a0});
    line(ctx, [tx, H * .3 + 7], [tx, H * .52 - 7], {alpha: .3});
    dot(ctx, tx, H * .52, 6, C.muted, {alpha: a0}); label(ctx, "Reviewer", tx + 10, H * .52, {size: 11, color: C.muted, halo: false, alpha: a0});
    line(ctx, [tx - 12, H * .22], [tx + 64, H * .6], {color: C.bad, alpha: .7, w: 1.6});
    label(ctx, word("notSubtype"), tx - 10, H * .7, {mono: true, size: 10, color: C.muted, halo: false});
    const Y = [W * .46, H * .4], A = [W * .86, H * .4], rel = win(t, .4, 1.3), end = win(t, 2.8, 3.6), fade = win(t, 3.6, 4.4);
    line(ctx, Y, A, {p: rel, alpha: rel > 0 ? mix(.85, .35, fade) : 0, color: fade > .5 ? C.muted : C.ink2, w: 1.4});
    if (rel > .5) label(ctx, "reviews", (Y[0] + A[0]) / 2, Y[1] - 12, {align: "center", size: 11, color: fade > .5 ? C.muted : C.ink2, halo: false});
    dot(ctx, Y[0], Y[1], 10, PERSON); label(ctx, word("you"), Y[0], Y[1] + 22, {align: "center", size: 12, weight: 600, color: C.ink, halo: false}); label(ctx, "Person", Y[0], Y[1] + 37, {align: "center", mono: true, size: 10, color: C.muted, halo: false});
    dot(ctx, A[0], A[1], 9, TYPE.App); label(ctx, "billing-api", A[0], A[1] + 22, {align: "center", size: 11.5, halo: false});
    const bx0 = W * .5, bx1 = W * .82, by = H * .8, grow = win(t, 1.3, 2.4);
    if (grow > 0) {
      ctx.globalAlpha = .8; ctx.fillStyle = C.ink2; rrect(ctx, bx0, by - 3, (end > 0 ? mix(bx1 + 14, bx1, end) : bx0 + (bx1 + 14 - bx0) * grow) - bx0, 6, 3); ctx.fill(); ctx.globalAlpha = 1;
      label(ctx, month(2), bx0, by + 14, {mono: true, size: 9.5, color: C.muted, halo: false});
      if (end > 0) label(ctx, month(7), bx1, by + 14, {align: "right", mono: true, size: 9.5, color: C.ink, halo: false, alpha: end});
      if (end > .5) line(ctx, [bx1, by - 7], [bx1, by + 7], {color: C.ink, alpha: end, w: 2});
    }
  },
};

// ---- P14 ----
SCENE[14] = {
  h: w => clamp(w * .7, 240, 280),
  draw(ctx, W, H, t) {
    const L = 14, R = W - 14, note = '"retries reuse the key"', id = "id " + (V(14).id || "");
    label(ctx, "laptop", L, 16, {size: 12, weight: 600, color: C.ink, halo: false});
    label(ctx, "desktop", R, 16, {align: "right", size: 12, weight: 600, color: C.ink, halo: false});
    const w1 = win(t, .3, .8), w2 = win(t, .6, 1.1);
    label(ctx, note, L, 38, {size: 11, alpha: w1, halo: false});
    label(ctx, note, R, 38, {align: "right", size: 11, alpha: w2, halo: false});
    const iw = chipW(ctx, id, {size: 10.5}), i1 = win(t, 1.1, 1.6), i2 = win(t, 1.3, 1.8), mg = win(t, 2.6, 3.4), y = 68;
    const xa = mix(L, W / 2 - iw / 2, mg), xb = mix(R - iw, W / 2 - iw / 2, mg);
    chip(ctx, id, xa, y, {size: 10.5, color: C.ink, alpha: i1});
    chip(ctx, id, xb, y, {size: 10.5, color: C.ink, alpha: i2 * (1 - (mg > .95 ? 1 : 0))});
    if (t > 1.9 && mg < .3) label(ctx, word("sameId"), W / 2, y, {align: "center", mono: true, size: 10, color: C.ink2, halo: false, alpha: win(t, 1.9, 2.2) * (1 - mg * 3)});
    if (mg > .9) label(ctx, word("oneEntry"), W / 2, y + 20, {align: "center", mono: true, size: 10, color: C.muted, halo: false, alpha: win(t, 3.3, 3.7)});
    const sy = H * .62;
    line(ctx, [14, sy - 26], [W - 14, sy - 26], {alpha: .2, w: 1});
    const G = [W * .26, sy + 18], Pg = [W * .7, sy + 18], m = win(t, 4.2, 5.0), q = win(t, 5.1, 5.7);
    const gp = [mix(G[0], Pg[0], m), mix(G[1], Pg[1], m)];
    dot(ctx, Pg[0], Pg[1], 10, TYPE.Database); label(ctx, "Postgres", Pg[0] + 15, Pg[1], {size: 12, weight: 600, color: C.ink});
    if (m < 1) { dot(ctx, gp[0], gp[1], 9, TYPE.Database, {alpha: 1 - m}); label(ctx, "pg", gp[0] - 13, gp[1], {align: "right", size: 12, weight: 600, color: C.ink, alpha: 1 - m}); }
    if (m > .95) {
      ctx.globalAlpha = .45; ctx.beginPath(); ctx.arc(G[0], G[1], 9, 0, 7); ctx.strokeStyle = C.muted; ctx.lineWidth = 1.2; ctx.stroke(); ctx.globalAlpha = 1;
      label(ctx, "pg", G[0] - 13, G[1], {align: "right", size: 12, color: C.muted, halo: false});
      if (q > 0) { arrowN(ctx, G, Pg, 12, 14, q, C.ink, 1.4); label(ctx, word("getPgForwarded"), (G[0] + Pg[0]) / 2, G[1] + 18, {align: "center", mono: true, size: 10, color: C.ink2, halo: false, alpha: q}); }
    }
    if (t > 4.1 && m < 1) label(ctx, word("merged"), (G[0] + Pg[0]) / 2, G[1] - 16, {align: "center", mono: true, size: 10, color: C.muted, halo: false, alpha: win(t, 4.1, 4.4)});
  },
};

// ---- P15 ----
SCENE[15] = {
  h: w => clamp(w * .62, 220, 260),
  draw(ctx, W, H, t) {
    const A = [W * .14, H * .2], G = [W * .36, H * .5], Pg = [W * .74, H * .5], r = 10;
    const p1 = win(t, .5, 1.1) * (1 - win(t, 2.5, 2.9)), m = win(t, 2.6, 3.6), p2 = win(t, 4.6, 5.1) * (1 - win(t, 5.6, 5.9)), s = win(t, 5.6, 6.5);
    const k = m * (1 - s), gp = [mix(G[0], Pg[0], k), mix(G[1], Pg[1], k)];
    line(ctx, A, gp);
    if (p1 > 0) { arrow(ctx, G, Pg, r, r, p1); label(ctx, word("sameThing"), (G[0] + Pg[0]) / 2, G[1] - 15, {align: "center", size: 11.5, alpha: p1}); }
    if (p2 > 0) { ctx.globalAlpha = p2; ctx.strokeStyle = C.gold; ctx.lineWidth = 1.2; ctx.beginPath(); ctx.arc(Pg[0], Pg[1], r + 9, 0, 7); ctx.stroke(); ctx.globalAlpha = 1; label(ctx, word("split"), Pg[0], Pg[1] - r - 20, {align: "center", size: 11.5, alpha: p2}); }
    dot(ctx, A[0], A[1], 9, TYPE.App); label(ctx, "billing-api", A[0] + 14, A[1], {size: 11.5});
    dot(ctx, Pg[0], Pg[1], r, TYPE.Database); label(ctx, "Postgres", Pg[0], Pg[1] + 22, {align: "center", size: 12, weight: 600, color: C.ink});
    if (k < 1) { dot(ctx, gp[0], gp[1], r, TYPE.Database, {alpha: 1 - k * .9}); label(ctx, "pg", gp[0], gp[1] + 22, {align: "center", size: 12, weight: 600, color: C.ink, alpha: 1 - k}); }
    const wy = H - 26, wx = 14, ww = W - 28; well(ctx, wx, wy - 10, ww, 20);
    [[.5, word("proposal"), C.muted], [2.6, word("merged"), C.ink], [4.6, word("splitProposal"), C.muted], [5.6, word("merged"), C.ink]].forEach(([at, s2, col], i) => {
      if (t < at) return; const x = wx + 10 + i * (ww - 20) / 3.2, fl = 1 - lin(t, at, at + .6);
      tick(ctx, x, wy, 6, 12, fl > .05 ? C.ink : col, .7 + .3 * fl);
      if (W > 330) label(ctx, s2, x + 8, wy, {mono: true, size: 9.5, color: C.muted, halo: false});
    });
  },
};
// ---- P17 ----
SCENE[17] = {
  h: w => clamp(w * .76, 250, 300),
  draw(ctx, W, H, t) {
    const lp = [12, 12, W * .58 - 12, H * .7], hb = [W * .66, 12, W * .34 - 12, H * .7];
    well(ctx, ...lp); well(ctx, ...hb);
    label(ctx, word("yourLaptop"), lp[0] + 10, lp[1] + 14, {mono: true, size: 10, color: C.muted, halo: false});
    label(ctx, word("teamHub"), hb[0] + 10, hb[1] + 14, {mono: true, size: 10, color: C.muted, halo: false});
    const rows = [["billing", 0], ["personal", 1], ["oss", 2]], ry = i => lp[1] + 44 + i * (lp[3] - 52) / 3;
    const shared = [0, null, 2.9], go = [[.6, 2.2], null, [3.2, 4.4]];
    rows.forEach(([nm, i]) => {
      const y = ry(i), pts = [0, 1, 2].map(j => [lp[0] + 22 + j * 16, y + (j % 2 ? 5 : -4)]);
      cloud(ctx, pts, 14, .8);
      pts.forEach(p => dot(ctx, p[0], p[1], 4.5, CONCEPT));
      label(ctx, nm, lp[0] + 76, y, {size: 11.5, color: C.ink, halo: false});
      const sh = shared[i] != null ? win(t, shared[i], shared[i] + .4) : 0;
      if (sh > 0) { if (lp[2] < 200) label(ctx, word("shared"), lp[0] + 76, y + 14, {mono: true, size: 9.5, color: C.gold, halo: false, alpha: sh}); else chip(ctx, word("shared"), lp[0] + lp[2] - 8, y, {align: "right", size: 9.5, color: C.ink, bg: C.select, alpha: sh}); }
      if (!go[i]) return;
      const [a, b] = go[i];
      pts.forEach((p, j) => {
        const k = win(t, a + j * .15, b + j * .15); if (k <= 0) return;
        const q = [hb[0] + 18 + j * 14, hb[1] + 44 + i * (lp[3] - 52) / 3 + (j % 2 ? 5 : -4)];
        const c = [mix(p[0], q[0], k), mix(p[1], q[1], k) - Math.sin(k * Math.PI) * 22];
        dot(ctx, c[0], c[1], 4.5, CONCEPT, {alpha: k < 1 ? 1 : 0});
        if (k >= 1) dot(ctx, q[0], q[1], 4.5, CONCEPT);
      });
      if (t >= b + .3 && hb[2] > 110) label(ctx, nm, hb[0] + 62, hb[1] + 44 + i * (lp[3] - 52) / 3, {size: 11, color: C.ink2, halo: false, alpha: win(t, b + .3, b + .6)});
    });
    const sy = H * .86, door = W * .5, s = lin(t, 4.8, 6.2), kw = chipW(ctx, "token=AKIA...EXAMPLE", {size: 10});
    line(ctx, [door, sy - 13], [door, sy + 13], {color: C.ink2, alpha: .5, w: 2.5});
    label(ctx, word("log"), door + 10, sy, {mono: true, size: 10, color: C.muted, halo: false});
    if (s > 0 && s < 1) {
      const gox = s < .45 ? ease(s / .45) : 1 - ease((s - .45) / .55) * .8;
      chip(ctx, "token=AKIA...EXAMPLE", mix(12, door - kw - 6, gox), sy, {size: 10, color: C.ink, alpha: 1 - lin(s, .85, 1)});
    }
    if (t > 5.4) label(ctx, word("refusedCredential"), 12, sy + 22, {size: 11, color: C.bad, halo: false, alpha: win(t, 5.4, 5.8)});
  },
};

// ---- P19 ----
SCENE[19] = {
  h: w => clamp(w * .7, 240, 280),
  draw(ctx, W, H, t) {
    const q = [14, 20], mx = 14, my = H * .44, off = win(t, 2.8, 3.3);
    chip(ctx, '"where do payments live?"', q[0], q[1], {sans: true, size: 11.5, color: C.ink});
    const mw = chipW(ctx, word("searchModel"), {size: 10.5});
    chip(ctx, word(off > .5 ? "searchModelOff" : "searchModel"), mx, my, {size: 10.5, color: off > .5 ? C.muted : C.ink, bg: off > .5 ? C.well : C.panel3});
    const N = {pg: [W * .74, H * .26], led: [W * .9, H * .5], api: [W * .6, H * .58], st: [W * .82, H * .76]};
    const NM = {pg: "Postgres", led: "ledger", api: "billing-api", st: "Stripe"}, KD = {pg: "Database", led: null, api: "App", st: null};
    line(ctx, N.api, N.pg); line(ctx, N.led, N.st);
    const wide = win(t, .5, 1.4) * (1 - off), kw = win(t, 3.3, 3.9);
    const src = [mx + mw, my];
    ["pg", "led", "st"].forEach((id, i) => { const a = wide * (i === 0 ? 1 : .85); if (a > 0) line(ctx, src, N[id], {color: C.ink2, alpha: .45 * a, p: win(t, .5 + i * .2, 1.2 + i * .2)}); glow(ctx, N[id][0], N[id][1], 20, C.ink, a * .9); });
    if (kw > 0) { line(ctx, [q[0] + 60, q[1] + 10], N.pg, {color: C.ink2, alpha: .55 * kw}); glow(ctx, N.pg[0], N.pg[1], 20, C.ink, kw); chip(ctx, word("keywordOnly"), mx, my + 28, {size: 10, color: C.ink2, alpha: kw}); }
    const pg2 = win(t, 4.8, 5.3), pr = win(t, 5.3, 5.9), PG2 = [W * .52, H * .3];
    for (const id in N) { const rt = N[id][0] > W * .8; dot(ctx, N[id][0], N[id][1], 7.5, KD[id] ? TYPE[KD[id]] : CONCEPT); label(ctx, NM[id], rt ? N[id][0] - 12 : N[id][0] + 12, N[id][1], {size: 11, align: rt ? "right" : "left"}); }
    if (pg2 > 0) { dot(ctx, PG2[0], PG2[1], 7.5 * pg2, TYPE.Database); label(ctx, "pg", PG2[0] - 12, PG2[1], {align: "right", size: 11.5, alpha: pg2}); }
    if (pr > 0) { arrow(ctx, PG2, N.pg, 8, 8, pr); label(ctx, word("sameThing"), PG2[0], PG2[1] - 18, {size: 11, alpha: pr}); label(ctx, word("waitsVerdict"), mx, H - 44, {mono: true, size: 10, color: C.muted, halo: false, alpha: win(t, 5.8, 6.2)}); }
    const wy = H - 18, ww = W * .42; well(ctx, 14, wy - 9, ww, 18);
    for (let i = 0; i < 5; i++) tick(ctx, 24 + i * 12, wy, 5, 9, C.ink2, .55);
    const wr = win(t, 3.8, 4.2), fl = 1 - lin(t, 4.2, 4.8);
    if (wr > 0) { tick(ctx, 24 + 5 * 12, wy, 5, 9, fl > .05 ? C.ink : C.ink2, wr); label(ctx, word("written"), 24 + 6 * 12 + 4, wy, {mono: true, size: 9.5, color: C.muted, halo: false, alpha: wr}); }
  },
};

// ---- P20 ----
SCENE[20] = {
  h: w => clamp(w * .72, 240, 290),
  draw(ctx, W, H, t) {
    const P = {cli: [.5, .09], mcp: [.32, .34], viz: [.7, .34], engine: [.15, .62], sync: [.39, .62], store: [.62, .62], embed: [.86, .62], core: [.5, .9]};
    const XY = id => [P[id][0] * W, P[id][1] * H];
    const D = [["cli", ["mcp", "viz", "engine", "sync", "store", "embed", "core"]], ["mcp", ["engine", "sync", "core"]], ["viz", ["engine"]], ["engine", ["core"]], ["sync", ["core"]], ["store", ["core"]], ["embed", ["core"]]];
    const layer = {cli: 0, mcp: 1, viz: 1, engine: 2, sync: 2, store: 2, embed: 2};
    D.forEach(([a, bs]) => bs.forEach((b, j) => {
      const k = win(t, .3 + layer[a] * .6 + j * .06, .9 + layer[a] * .6 + j * .06); if (k <= 0) return;
      const pa = XY(a), pb = XY(b), faint = a === "cli";
      arrowN(ctx, pa, [mix(pa[0], pb[0], k), mix(pa[1], pb[1], k)], 11, k < 1 ? 0 : 11, faint ? .28 : .6, C.ink2, 1.1);
      if (t > 1.2 && t < 2.9 && !faint) { const u = ((t * .7 + j * .3 + layer[a] * .2) % 1); const pt = [mix(pa[0], pb[0], u), mix(pa[1], pb[1], u)]; dot(ctx, pt[0], pt[1], 1.8, C.ink, {alpha: .8}); }
    }));
    const no = win(t, 2.9, 3.5), memK = win(t, 4.6, 5.3);
    if (no > 0) {
      const a = XY("core"), b = XY("store"), mid = [mix(a[0], b[0], .5), mix(a[1], b[1], .5)];
      arrowN(ctx, a, [mix(a[0], b[0], no), mix(a[1], b[1], no)], 12, no < 1 ? 0 : 12, .9, C.bad, 1.4);
      const xa = win(t, 3.4, 3.7);
      line(ctx, [mid[0] - 6, mid[1] - 6], [mid[0] + 6, mid[1] + 6], {color: C.bad, alpha: xa, w: 2.2}); line(ctx, [mid[0] + 6, mid[1] - 6], [mid[0] - 6, mid[1] + 6], {color: C.bad, alpha: xa, w: 2.2});
    }
    for (const id in P) {
      const [x, y] = XY(id), isCore = id === "core";
      if (id === "store" && memK > 0) {
        chip(ctx, "store", x, y, {align: "center", size: 10.5, alpha: 1 - memK});
        chip(ctx, word("inMemory"), x, y, {align: "center", size: 10.5, color: C.ink, bg: C.select, alpha: memK});
      } else chip(ctx, id, x, y, {align: "center", size: 10.5, color: isCore ? C.bg : C.ink2, bg: isCore ? C.ink : C.panel3, weight: isCore ? 600 : 500});
    }
  },
};

// ---- P21 ----
SCENE[21] = {
  h: w => w < 420 ? clamp(w * 1.08, 270, 340) : clamp(w * .7, 250, 290),
  draw(ctx, W, H, t) {
    const T = ["define_type", "get_entity", "get_proposal", "list_proposals", "observe", "propose", "review", "search_knowledge", "sync_pull", "sync_push", "sync_status", "traverse", "workspace_map"];
    const two = W >= 420, x0 = W * (two ? .38 : .5), colW = two ? (W - x0 - 12) / 2 : W - x0 - 12, rows = two ? 7 : 13, top = 16, rh = two ? (H - 36) / 7 : (H - 30) / 13;
    const pos = i => { const c = two ? (i < 7 ? 0 : 1) : 0, r = two ? i % 7 : i; return [x0 + c * colW, top + r * rh + rh / 2]; };
    const M = [W * .1, H * .46];
    dot(ctx, M[0], M[1], 9, C.teal); label(ctx, word("aModel"), M[0], M[1] + 21, {align: "center", size: 11.5, halo: false});
    const call = (i, a, b) => { const k = lin(t, a, b); if (k <= 0 || k >= 1) return -1; const [x, y] = pos(i), p = travel([M, [x - 6, y], M], k); dot(ctx, p[0], p[1], 3.5, C.ink); return k; };
    const hlS = t > 3.0 && t < 4.5 ? 1 : 0, hlO = 0;
    T.forEach((s, i) => {
      const a = win(t, .2 + i * .17, .5 + i * .17), on = (i === 7 && hlS) || (i === 4 && hlO);
      chip(ctx, s, pos(i)[0], pos(i)[1], {size: two ? 10 : 9.5, color: on ? C.ink : C.ink2, bg: on ? C.select : C.panel3, alpha: a});
    });
    if (t > 2.4) label(ctx, word("tools13"), M[0], M[1] - 22, {align: "center", mono: true, size: 10, color: C.muted, halo: false, alpha: win(t, 2.4, 2.8)});
    call(7, 3.0, 4.1);
    const ob = win(t, 4.4, 4.7) * (1 - win(t, 5.9, 6.2)), O = [12, H * .66];
    if (ob > 0) chip(ctx, word("observation9b1e"), O[0], O[1], {size: 9.5, color: C.ink2, alpha: ob});
    const fk = lin(t, 4.6, 5.3);
    if (fk > 0 && fk < 1) { const p = travel([M, [O[0] + 40, O[1] - 8], M], fk); dot(ctx, p[0], p[1], 3.5, C.ink); }
    const er = win(t, 5.2, 5.6) * (1 - win(t, 5.9, 6.2));
    if (er > 0) ["not found: 9b1e.", "absence is not", "a negation; use an", "id from a search"].forEach((s, i) => label(ctx, s, 12, H * .66 + 18 + i * 13, {mono: true, size: 9.5, color: C.bad, halo: false, alpha: er}));
    const qk = lin(t, 5.9, 7.0);
    if (qk > 0) {
      const qw = chipW(ctx, "MATCH (n) RETURN n", {size: 9.5}), x = mix(12, Math.max(12, x0 - qw - 6), ease(Math.min(1, qk * 1.5)));
      chip(ctx, "MATCH (n) RETURN n", x, H * .8, {size: 9.5, color: C.ink2, bg: C.well, alpha: 1 - lin(qk, .85, 1) * .5});
      if (qk > .6) label(ctx, word("noPassthrough"), 12, H * .8 + 20, {mono: true, size: 10, color: C.muted, halo: false, alpha: win(t, 6.6, 6.9)});
    }
  },
};

// ---- P22 ----
SCENE[22] = {
  h: w => w < 420 ? clamp(w * 1.12, 280, 350) : clamp(w * .72, 250, 290),
  draw(ctx, W, H, t) {
    const tx = 12, ty = 12, tw = W - 24, th = H - 58, font = `400 10.5px ${MONO}`, lh = 14.5, maxW = tw - 22;
    well(ctx, tx, ty, tw, th);
    const L1 = [
      {at: .2, dur: 1.0, s: '$ git commit -m "Retry webhooks with idempotency keys"', c: C.ink},
      {at: 1.4, dur: 0, s: "hook: commit 3fa91c2 landed in billing. If it settled a decision the diff does not show, record it now with observe. If it settled nothing, record nothing.", c: C.muted},
      {at: 2.8, dur: 1.0, s: 'agent: observe "A retry reuses the idempotency key, so a replay never charges twice" source git:3fa91c2', c: C.ink2},
    ];
    const L2 = [
      {at: 5.0, dur: 0, s: word("newSession"), c: C.muted},
      {at: 5.3, dur: 0, s: 'supragnosis: workspace "billing" holds 13 entities and 7 relations on this node. Search it before assuming what this project has decided.', c: C.ink2},
    ];
    const clear = win(t, 4.7, 5.0), set = t < 5.0 ? L1 : L2, fade = t < 5.0 ? 1 - clear : 1;
    let y = ty + 16;
    set.forEach(l => {
      if (t < l.at) return;
      const n = l.dur ? Math.floor(l.s.length * lin(t, l.at, l.at + l.dur)) : l.s.length;
      const lines = wrapLines(ctx, l.s, maxW, font);
      let left = n;
      lines.forEach(s => { const part = s.slice(0, Math.max(0, left)); left -= s.length + 1; if (part) label(ctx, part, tx + 11, y, {mono: true, size: 10.5, weight: 400, color: l.c, halo: false, alpha: fade * (l.dur ? 1 : win(t, l.at, l.at + .3))}); y += lh; });
      y += 5;
    });
    const wy = H - 22, ww = W - 24; well(ctx, 12, wy - 10, ww, 20);
    label(ctx, word("log"), 20, wy, {mono: true, size: 10, color: C.muted, halo: false});
    for (let i = 0; i < 6; i++) tick(ctx, 50 + i * 12, wy, 5, 10, C.ink2, .5);
    const nw = win(t, 3.9, 4.3), fl = 1 - lin(t, 4.3, 5.0);
    if (nw > 0) { tick(ctx, 50 + 6 * 12, wy, 5, 10, fl > .05 ? C.ink : C.ink2, nw); glow(ctx, 50 + 72, wy, 18, C.teal, fl); label(ctx, "git:3fa91c2", 50 + 7 * 12 + 2, wy, {mono: true, size: 9.5, color: C.muted, halo: false, alpha: nw}); }
  },
};

// ---- P24 ----
SCENE[24] = {
  h: w => clamp(w * .74, 250, 296),
  draw(ctx, W, H, t) {
    const refuse = win(t, 5.0, 5.5), offM = win(t, .5, 1.0), offS = win(t, 2.6, 3.1);
    const st = word(refuse > .5 ? "refusedStart" : "serving");
    chip(ctx, st, 14, 18, {size: 11, color: refuse > .5 ? C.bad : C.ink, bg: refuse > .5 ? C.well : C.panel3});
    const R = [[word("rStore"), null], [word("rKeyword"), null], [word("rSemantic"), [offM, word("offModel")]], [word("rSync"), [offS, word("offHub")]], [word("rViewer"), null]];
    const y0 = 50, rh = (H - 50 - 44) / 5;
    R.forEach(([nm, off], i) => {
      const y = y0 + i * rh, k = off ? off[0] : 0, dim = 1 - .55 * refuse;
      ctx.globalAlpha = dim;
      if (k > .5) { ctx.beginPath(); ctx.arc(20, y, 4.5, 0, 7); ctx.strokeStyle = C.muted; ctx.lineWidth = 1.4; ctx.stroke(); }
      else dot(ctx, 20, y, 4.5, C.ink);
      ctx.globalAlpha = 1;
      label(ctx, nm, 32, y, {size: 11.5, color: k > .5 ? C.muted : C.ink, halo: false, alpha: dim});
      if (off && k > 0) label(ctx, off[1], 32, y + 14, {mono: true, size: 9.5, color: C.ink2, halo: false, alpha: k * dim});
      if (off && t > (i === 2 ? .5 : 2.6) && t < (i === 2 ? 1.6 : 3.7)) glow(ctx, 20, y, 18, C.ink, Math.sin(lin(t, i === 2 ? .5 : 2.6, i === 2 ? 1.6 : 3.7) * Math.PI));
    });
    const c = win(t, 4.6, 5.0);
    if (c > 0) { const y = H - 20; chip(ctx, word("configKey"), 14, y, {size: 10, color: C.bad, bg: C.well, alpha: c}); }
  },
};
// ---- the ledger: pressing a column shows that principle's clauses beside it ----
function ledger() {
  const detail = $("#detail"), cols = $$(".sky .col");
  if (!detail || !cols.length) return;
  function pick(a) {
    cols.forEach(c => c.setAttribute("aria-current", c === a ? "true" : "false"));
    const p = a.dataset.p, art = $("#p" + p);
    if (!art) return;
    const tally = $(".clauses .tally", art), list = $(".clauses ul", art);
    const dl = el("p", {class: "dl"}, tally ? tally.textContent + ". " : ""), link = el("a", {href: "#p" + p}, word("readPrinciple"));
    dl.append(link);
    detail.replaceChildren(el("span", {class: "dn"}, "P" + p), el("p", {class: "dt"}, $("h3", art).textContent), dl, ...(list ? [list.cloneNode(true)] : []));
  }
  cols.forEach(a => a.addEventListener("click", e => { if (window.matchMedia("(max-width: 899px)").matches) return; e.preventDefault(); pick(a); }));
  pick(cols.find(c => c.dataset.p === "3") || cols[0]);
}

// ---- the tensions, in marks of the page's own: a chapter's ground, a principle's neutral dot, a
// ruling's diamond, and a label for a side that is not a principle (Section 11a) ----
function tensions() {
  const cv = $(".tmap"), items = $$(".tlist li");
  if (!cv || !items.length) return;
  const chapters = $$("section.chap").map(sec => ({name: ($(".eyebrow span", sec) || {}).textContent || "", ps: $$("article.pr", sec).map(a => +a.id.slice(1))}));
  const nums = v => (v || "").split(" ").filter(Boolean).map(Number);
  const TEN = items.map(li => ({a: nums(li.dataset.a), b: nums(li.dataset.b), side: li.dataset.side || "", owed: li.hasAttribute("data-owed"), li}));
  const jd = el("div", {class: "jd", "aria-live": "off"});
  cv.after(jd);
  let sel = Math.max(0, TEN.findIndex(T => $("button", T.li).getAttribute("aria-pressed") === "true")), geo = null;
  const RAD = [.15, .13, .12, .105, 0, 0];
  function layout(W, H) {
    const tall = W < 480, m = Math.min(W, H), pos = {};
    const C6 = tall ? [[.3, .14], [.7, .3], [.3, .47], [.7, .63], [.3, .8], [.7, .9]] : [[.17, .35], [.47, .22], [.8, .33], [.25, .77], [.55, .72], [.84, .77]];
    const centers = C6.map(([x, y]) => [x * W, y * H]);
    chapters.forEach((ch, c) => { const [cx, cy] = centers[c], n = ch.ps.length; ch.ps.forEach((p, i) => { const ang = -Math.PI / 2 + i * 2 * Math.PI / n + (c % 2 ? .3 : 0), R = RAD[c] * m; pos[p] = [cx + Math.cos(ang) * R * (tall ? 1 : 1.25), cy + Math.sin(ang) * R]; }); });
    const dia = TEN.map(T => {
      const ps = T.a.concat(T.b).filter(p => pos[p]);
      if (ps.length > 1) { let x = 0, y = 0; ps.forEach(p => { x += pos[p][0]; y += pos[p][1]; }); return [x / ps.length, y / ps.length]; }
      const [px, py] = pos[ps[0]], dx = px - W / 2, dy = py - H / 2, d = Math.hypot(dx, dy) || 1;
      return [px + dx / d * 34, py + dy / d * 34];
    });
    for (let it = 0; it < 80; it++) {
      for (let i = 0; i < dia.length; i++) for (let j = i + 1; j < dia.length; j++) {
        const dx = dia[j][0] - dia[i][0], dy = dia[j][1] - dia[i][1], d = Math.hypot(dx, dy) || .01;
        if (d < 26) { const s2 = (26 - d) / 2; dia[i][0] -= dx / d * s2; dia[i][1] -= dy / d * s2; dia[j][0] += dx / d * s2; dia[j][1] += dy / d * s2; }
      }
      for (const b of dia) for (const p in pos) { const dx = b[0] - pos[p][0], dy = b[1] - pos[p][1], d = Math.hypot(dx, dy) || .01; if (d < 24) { b[0] += dx / d * (24 - d); b[1] += dy / d * (24 - d); } }
      for (const b of dia) { b[0] = clamp(b[0], 16, W - 16); b[1] = clamp(b[1], 16, H - 16); }
    }
    return {pos, centers, dia, tall, m};
  }
  function diamond(ctx, x, y, r, fill, a) {
    ctx.globalAlpha = a; ctx.beginPath(); ctx.moveTo(x, y - r); ctx.lineTo(x + r, y); ctx.lineTo(x, y + r); ctx.lineTo(x - r, y); ctx.closePath();
    ctx.fillStyle = fill; ctx.fill(); ctx.lineWidth = 1.5; ctx.strokeStyle = C.bg; ctx.stroke(); ctx.globalAlpha = 1;
  }
  const S = surface(cv, w => w < 480 ? w * 1.3 : clamp(w * .5, 300, 480), s => {
    const {ctx, W, H} = s; geo = layout(W, H); const {pos, centers, dia, tall, m} = geo;
    chapters.forEach((ch, c) => {
      const [cx, cy] = centers[c], R0 = RAD[c] * m, rx = R0 * (tall ? 1 : 1.25) + 30, ry = R0 + 26;
      ctx.fillStyle = C.well; rrect(ctx, cx - rx, cy - ry, rx * 2, ry * 2, 18); ctx.fill();
      label(ctx, ch.name.toUpperCase(), cx, cy + ry + 12, {align: "center", mono: true, size: 10, color: C.muted, halo: false});
    });
    const hot = new Set(TEN[sel].a.concat(TEN[sel].b));
    TEN.forEach((T, i) => { const on = i === sel; T.a.concat(T.b).forEach(p => { if (pos[p]) line(ctx, dia[i], pos[p], {alpha: on ? .9 : .2, color: on ? C.ink : C.muted, w: on ? 1.6 : 1}); }); });
    for (const p in pos) {
      const on = hot.has(+p);
      dot(ctx, pos[p][0], pos[p][1], on ? 8 : 6, on ? C.ink : C.ink2, {alpha: on ? 1 : .75});
      label(ctx, "P" + p, pos[p][0] + (on ? 12 : 10), pos[p][1], {mono: true, size: on ? 11.5 : 10.5, weight: on ? 600 : 500, color: on ? C.ink : C.muted});
    }
    TEN.forEach((T, i) => {
      const on = i === sel, [x, y] = dia[i];
      diamond(ctx, x, y, on ? 9 : 6, on ? C.ink : C.ink2, T.owed && !on ? .5 : 1);
      if (on && T.side) label(ctx, T.side, x, y + 20, {align: "center", size: 11, color: C.ink});
    });
  });
  function pick(i, pressed) {
    sel = i;
    TEN.forEach((T, j) => $("button", T.li).setAttribute("aria-pressed", String(j === i)));
    const li = TEN[i].li;
    if (pressed) jd.setAttribute("aria-live", "polite");
    const h = el("h3"); h.append(cloneKids($(".tn", li)));
    jd.replaceChildren($(".vs", li).cloneNode(true), h, $(".win", li).cloneNode(true));
    S.redraw();
  }
  cv.addEventListener("click", e => {
    const r = cv.getBoundingClientRect(), x = e.clientX - r.left, y = e.clientY - r.top;
    let best = -1, bd = 20; geo.dia.forEach(([bx, by], i) => { const d = Math.hypot(bx - x, by - y); if (d < bd) { bd = d; best = i; } });
    if (best >= 0) pick(best, true);
  });
  TEN.forEach((T, i) => $("button", T.li).addEventListener("click", () => pick(i, true)));
  pick(sel, false);
}

// ---- one principle at a time (Section 11a, step 67): the chapters are an index of rows, each title
// opens its principle, and an opened one is the only one on the screen, with a bar that says where it
// is and steps through the others in the document's order. The anchors are the page's own, so a link
// to #p3 opens P3 and the back button goes back. ----
function reader() {
  const main = $("main"), arts = $$("article.pr");
  if (!main || !arts.length) return;
  const TITLE = document.title, title = a => $("h3", a).textContent;
  arts.forEach(a => {
    const h = $("h3", a), open = el("a", {href: "#" + a.id, class: "open"});
    open.append(...h.childNodes); h.append(open); a.setAttribute("tabindex", "-1");
    // a demo stands beside its text on the two-column page; read alone it comes before the clauses,
    // in the markup too, so the focus goes in the order the eye does
    const pt = $(".pt", a), fig = $(":scope > figure", a);
    if (pt && fig) pt.insertBefore(fig, $(".clauses, .refs", pt));
  });
  const bar = el("nav", {class: "reader", "aria-label": word("principles")});
  const back = el("a", {class: "back"}, word("allPrinciples")), where = el("span", {class: "where"});
  const prev = el("a", {class: "turn", rel: "prev"}), next = el("a", {class: "turn", rel: "next"});
  bar.append(back, where, prev, next);
  let last = null;
  function to(link, a, w) {
    link.hidden = !a;
    if (!a) return;
    link.href = "#" + a.id;
    link.replaceChildren(el("b", null, w), el("span", null, "P" + a.id.slice(1) + " " + title(a)));
  }
  function route() {
    const id = location.hash.slice(1), i = arts.findIndex(a => a.id === id), a = arts[i];
    const named = id ? document.getElementById(id) : null;
    // the skip link, or a link inside the open principle, leaves it open
    if (!a && last && named && (named === main || last.contains(named))) return;
    arts.forEach(x => x.classList.toggle("open", x === a));
    $$("section.chap").forEach(c => c.classList.toggle("here", !!a && c.contains(a)));
    if (!a) {
      main.dataset.view = "index";
      document.title = TITLE;
      bar.remove();
      // back from a principle, the index shows it where the reader left it, unless the address names
      // another place; the page is replaced, so it does not glide there
      const target = last && (!named || named.contains(last)) ? last : named;
      if (target) setTimeout(() => target.scrollIntoView({block: target === last ? "center" : "start", behavior: "instant"}), 0);
      if (target && target === last) $("h3 a.open", last).focus({preventScroll: true});
      last = null;
      return;
    }
    main.dataset.view = "one";
    where.textContent = word("whereOf", {p: a.id.slice(1), n: arts.length});
    back.href = "#" + a.closest("section.chap").id;
    to(prev, arts[i - 1], word("previous"));
    to(next, arts[i + 1], word("next"));
    $(".chap-head", a.closest("section.chap")).before(bar);
    // the focus goes to the principle, named by its title, which is where the browser's own jump to
    // the anchor puts it too; that jump lands at the top of the page (style.css), and a step from
    // another principle goes there at once rather than gliding over a page that was replaced
    a.focus({preventScroll: true});
    setTimeout(() => window.scrollTo({top: 0, behavior: "instant"}), 0);
    document.title = title(a) + " - " + TITLE;
    last = a;
  }
  window.addEventListener("hashchange", route);
  // the arrow keys step and Escape goes back, unless a control has the focus
  document.addEventListener("keydown", e => {
    if (main.dataset.view !== "one" || e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
    if (e.target.closest("button, input, select, textarea, summary")) return;
    const k = e.key === "ArrowRight" ? next : e.key === "ArrowLeft" ? prev : e.key === "Escape" ? back : null;
    if (k && !k.hidden) { e.preventDefault(); location.hash = k.getAttribute("href"); }
  });
  route();
}

// ---- boot: each demo and scene is set up as it nears the window; one that cannot draw reads as text ----
function nearView(elm, fn) {
  const io = new IntersectionObserver(es => { if (es.some(e => e.isIntersecting)) { io.disconnect(); fn(); } }, {rootMargin: "600px 0px"});
  io.observe(elm);
}
function guarded(fig, fn) { return () => { try { fn(); } catch (e) { fig.classList.add("still"); } }; }
// A caption changes as a figure plays on its own, so it is announced only once the reader has pressed
// one of the figure's controls (Section 11a).
function listenAfterPress(fig) {
  fig.addEventListener("click", e => { if (e.target.closest("button")) $$(".cap", fig).forEach(c => c.setAttribute("aria-live", "polite")); }, true);
  $$(".cap", fig).forEach(c => c.setAttribute("aria-live", "off"));
}
function boot() {
  if (!window.ResizeObserver || !window.IntersectionObserver || !document.createElement("canvas").getContext) { document.documentElement.classList.remove("js"); return; }
  try { reader(); } catch (e) { /* the whole page reads as it does without a script */ }
  try { ledger(); } catch (e) { /* the columns stay links to their principles */ }
  // a canvas does not make the browser fetch the Hangul a unicode-range holds back, so on a Korean
  // page the figures wait for the faces of the words they draw: the words table, and what they take
  // from the page, a chapter's name and the words a tension or a question carries (docs/i18n.md
  // Section 7); the browser fetches the faces of the page's own text as it lays it out
  const drawn = () => Object.values(WORDS).map(e => e.ko).join("")
    + $$(".chap .eyebrow span").map(s => s.textContent).join("")
    + $$("[data-side], [data-route]").map(e => (e.dataset.side || "") + (e.dataset.route || "")).join("");
  const faces = LANG === "ko" && document.fonts && document.fonts.load
    ? document.fonts.load(`11px ${SANS}`, drawn()).catch(() => {})
    : Promise.resolve();
  faces.then(() => {
    $$("figure.scene").forEach(fig => { const p = +fig.dataset.scene; if (SCENE[p]) nearView(fig, guarded(fig, () => { mountScene(p, fig); listenAfterPress(fig); })); else fig.classList.add("still"); });
    $$("figure.demo").forEach(fig => { const p = +fig.dataset.demo; if (MOUNT[p]) nearView(fig, guarded(fig, () => { listenAfterPress(fig); MOUNT[p](fig); })); else fig.classList.add("still"); });
    try { tensions(); } catch (e) { const s = $(".tensions"); if (s) s.classList.add("still"); }
  });
}
if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", boot); else boot();
})();
