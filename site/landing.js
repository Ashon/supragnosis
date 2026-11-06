// The landing's controls: its tab lists, its Copy buttons and the steps' choices on the stage. The
// English page and its Korean twin (site/ko/) load this one file, so the words it writes are kept
// here, one entry per sentence in both languages, and picked by the page's `lang` (docs/i18n.md
// Section 4). Loaded with `defer`, after stage.js, so it runs once the page is parsed.
(function () {
  var LANG = document.documentElement.lang === "ko" ? "ko" : "en";
  var WORDS = {
    copy: { en: "Copy", ko: "복사" },
    copied: { en: "Copied", ko: "복사됨" },
    copiedCommand: { en: "Copied the command", ko: "명령을 복사했습니다" },
    caption0: { en: "a made-up project, three agents and you", ko: "지어낸 프로젝트, 에이전트 셋과 나" },
    caption1: { en: "1 write", ko: "1 쓰기" },
    caption2: { en: "2 structure", ko: "2 구조화" },
    caption3: { en: "3 govern", ko: "3 판정" },
    caption4: { en: "4 recall", ko: "4 회상" },
    caption5: { en: "5 travel", ko: "5 이동" },
    allEntries: { en: "all {n} entries", ko: "항목 {n}개 모두" },
    someEntries: { en: "{k} of {n} entries", ko: "항목 {n}개 중 {k}개" },
    kOfN: { en: "{k} of {n}", ko: "{k} / {n}" },
    graph: { en: "{entities} entities, {relations} relations", ko: "개체 {entities}개, 관계 {relations}개" },
    notYet: { en: "not yet", ko: "아직 없음" },
    accept: { en: "Accept", ko: "수락" },
    accepted: { en: "Accepted", ko: "수락함" },
    doneMerge: { en: "Merged pg into postgres", ko: "pg를 postgres로 병합함" },
    doneConfirm: { en: "Confirmed: webhooks need an idempotency key", ko: "확인함: webhooks need an idempotency key" },
    saidMerge: { en: "Merged pg into postgres. The verdict is an entry in the log.", ko: "pg를 postgres로 병합했습니다. 판정은 로그의 한 항목입니다." },
    saidConfirm: { en: "Confirmed: webhooks need an idempotency key. The verdict is an entry in the log.", ko: "확인했습니다: webhooks need an idempotency key. 판정은 로그의 한 항목입니다." },
    humanConfirmed: { en: "human confirmed", ko: "사람이 확인" },
    agent: { en: "agent", ko: "에이전트" },
    hitSaid: { en: "{name}: entries {entries}, {tier}.", ko: "'{name}': 항목 {entries}, {tier}." },
    trust: { en: "{tier}", ko: "{tier}" },
    laptopBare: { en: "{tier}, words in an entry are not a verdict", ko: "{tier}, 항목에 적힌 말은 판정이 아님" },
    laptopVerdict: { en: "{tier}, in its Review", ko: "{tier}, 그 기기의 검토에서" },
    desktopBare: { en: "{tier}, the same entry", ko: "{tier}, 같은 항목" },
    desktopVerdict: { en: "{tier}, the same verdict", ko: "{tier}, 같은 판정" },
    saidBare: { en: "An agent saying so: agent on both machines. Only a verdict in Review confirms.", ko: "에이전트의 말일 뿐이라 두 기기 모두 신뢰 등급이 '에이전트'입니다. 확인은 검토의 판정으로만 됩니다." },
    saidVerdict: { en: "The verdict: human confirmed on both machines.", ko: "판정이 있으니 두 기기 모두 신뢰 등급이 '사람이 확인'입니다." },
  };
  // A sentence with its named placeholders filled; an entry is never joined to another.
  function t(key, values) {
    return WORDS[key][LANG].replace(/\{(\w+)\}/g, function (_, name) { return String(values[name]); });
  }

  // Tab lists: click or the arrow keys, one tab in the tab order (design-system.md Section 10).
  document.querySelectorAll('[role="tablist"]').forEach(function (list) {
    var tabs = Array.prototype.slice.call(list.querySelectorAll('[role="tab"]'));
    function select(tab) {
      tabs.forEach(function (tb) {
        var on = tb === tab;
        tb.setAttribute("aria-selected", on ? "true" : "false");
        tb.tabIndex = on ? 0 : -1;
        document.getElementById(tb.getAttribute("aria-controls")).hidden = !on;
      });
    }
    tabs.forEach(function (tb, i) {
      tb.addEventListener("click", function () { select(tb); });
      tb.addEventListener("keydown", function (e) {
        if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
        var next = tabs[(i + (e.key === "ArrowRight" ? 1 : tabs.length - 1)) % tabs.length];
        next.focus(); select(next);
      });
    });
    // A visitor on Linux sees the Linux tab first.
    if (!/Mac/i.test(navigator.platform || navigator.userAgent)) {
      var linux = list.querySelector("#tab-linux, #hero-tab-script");
      if (linux) select(linux);
    }
  });

  // Copy a command; where the clipboard is refused, select it so it can be copied by hand.
  document.querySelectorAll(".copy").forEach(function (btn) {
    btn.addEventListener("click", function () {
      // An install command's copy is counted, with which command it was, as the Privacy page says
      // (analytics.js); the check's is not, since it installs nothing.
      if (window.gtag && btn.dataset.method) window.gtag("event", "copy_install", { method: btn.dataset.method });
      var el = document.querySelector(btn.dataset.copy);
      var pick = function () { getSelection().selectAllChildren(el); };
      var done = function () {
        btn.textContent = t("copied");
        // Emptied first, so a second copy is announced again.
        document.getElementById("copied").textContent = "";
        setTimeout(function () { document.getElementById("copied").textContent = t("copiedCommand"); }, 50);
        setTimeout(function () { btn.textContent = t("copy"); }, 1600);
      };
      try { navigator.clipboard.writeText(el.textContent).then(done, pick); } catch (e) { pick(); }
    });
  });

  // The stage (design-system.md Section 11): beside the steps from 900 pixels, a frame per step below.
  if (!window.Stage || !window.ResizeObserver || !window.IntersectionObserver || !Intl.ListFormat) {
    document.documentElement.classList.remove("js");
    return;
  }
  var Stage = window.Stage;
  var reduce = matchMedia("(prefers-reduced-motion: reduce)");
  var wide = matchMedia("(min-width: 900px)");
  var steps = Array.prototype.slice.call(document.querySelectorAll(".step"));
  var rail = Array.prototype.slice.call(document.querySelectorAll(".rail [data-go]"));
  var caption = document.getElementById("stage-caption");
  var stage;
  try { stage = Stage.mount(document.getElementById("stage")); } catch (e) {
    // a stage that cannot draw leaves its still picture and the steps (design-system.md Section 11)
    document.documentElement.classList.remove("js");
    return;
  }

  function show(i) {
    if (!wide.matches) return;
    stage.go(i);
    rail.forEach(function (b) {
      if (+b.dataset.go === i) b.setAttribute("aria-current", "step");
      else b.removeAttribute("aria-current");
    });
    caption.textContent = t("caption" + i);
  }
  // The step whose box crosses the middle of the window is the current one. Nothing reads the
  // scroll position, and the scroll is never held.
  var middle = new IntersectionObserver(function (entries) {
    entries.forEach(function (e) { if (e.isIntersecting) show(+e.target.dataset.scene); });
  }, { rootMargin: "-50% 0px -50% 0px" });
  steps.forEach(function (s) { middle.observe(s); });
  rail.forEach(function (b) {
    b.addEventListener("click", function () {
      var i = +b.dataset.go;
      document.getElementById("step-" + i).scrollIntoView({ block: "center", behavior: reduce.matches ? "auto" : "smooth" });
      show(i);
    });
  });
  document.getElementById("stage-replay").addEventListener("click", function () { reset(stage.scene); stage.replay(); });

  // Below 900 pixels each step draws its own frame, which plays its scene once it is mostly in view.
  var frames = {};
  steps.forEach(function (s) {
    var c = s.querySelector(".frame");
    if (!c) return;
    var i = +s.dataset.scene, frame = frames[i] = Stage.mount(c, { scene: i - 1 });
    s.querySelector(".replay").addEventListener("click", function () {
      if (frame.scene === i) reset(i);
      frame.go(i, frame.scene === i);
    });
    new IntersectionObserver(function (es) {
      if (es[0].isIntersecting && frame.scene !== i && !wide.matches) frame.go(i);
    }, { threshold: 0.6 }).observe(c);
  });

  // The steps' controls (design-system.md Section 11, step 66). Each casts its choice on the stage
  // that shows its step - beside the steps, or the step's own frame - and the step's words say the
  // result, so a reader without the canvas reads it too.
  var said = document.getElementById("stage-said");
  // what the visitor chose, per scene, so a stage that comes into use (crossing 900 pixels) shows it
  var picked = {};
  function pick(i, key, value) { (picked[i] = picked[i] || {})[key] = value; picked[i][key + "At"] = 0; }
  function on(i) {
    if (wide.matches) { if (stage.scene !== i) show(i); return stage; }
    if (frames[i].scene !== i) frames[i].go(i);
    return frames[i];
  }
  wide.addEventListener("change", function () {
    Object.keys(picked).forEach(function (i) { (wide.matches ? stage : frames[i]).set(+i, picked[i]); });
  });
  function say(text) { said.textContent = ""; setTimeout(function () { said.textContent = text; }, 50); }
  function byId(id) { return document.getElementById(id); }
  // en-GB, since the site writes no comma before "and"
  var list = new Intl.ListFormat(LANG === "ko" ? "ko" : "en-GB", { type: "conjunction" });
  function and(items) { return list.format(items.map(String)); }
  // An entry with its {tier} drawn as the trust word, in gold when a person confirmed it.
  function tier(el, gold, key) {
    var parts = WORDS[key][LANG].split("{tier}");
    var tw = document.createElement("span");
    tw.textContent = gold ? t("humanConfirmed") : t("agent");
    if (gold) tw.className = "confirmed";
    el.textContent = "";
    el.appendChild(document.createTextNode(parts[0]));
    el.appendChild(tw);
    el.appendChild(document.createTextNode(parts[1]));
  }

  // 2: the log replayed to k entries
  var k = byId("replay-k");
  function replayed(n) {
    var c = Stage.counts(n);
    k.value = n;
    k.setAttribute("aria-valuetext", n === 6 ? t("allEntries", { n: 6 }) : t("someEntries", { k: n, n: 6 }));
    byId("replay-k-out").textContent = t("kOfN", { k: n, n: 6 });
    byId("replay-graph").textContent = t("graph", c);
    byId("replay-region").textContent = c.region ? "webhooks + idempotency" : t("notYet");
    return c;
  }
  k.addEventListener("input", function () { replayed(+k.value); pick(2, "k", +k.value); on(2).choose("k", +k.value); });

  // 3: the verdicts wait for Accept
  var acts = Array.prototype.slice.call(document.querySelectorAll("[data-act]")), waiting = 3;
  var DONE = { merge: "doneMerge", confirm: "doneConfirm" };
  var SAID = { merge: "saidMerge", confirm: "saidConfirm" };
  acts.forEach(function (b) {
    var row = byId("row-" + b.dataset.act);
    row.dataset.was = row.textContent;
    b.addEventListener("click", function () {
      if (b.getAttribute("aria-disabled")) return;
      pick(3, b.dataset.act, 0);
      on(3).choose(b.dataset.act);
      b.setAttribute("aria-disabled", "true");
      b.textContent = t("accepted");
      row.textContent = t(DONE[b.dataset.act]);
      byId("needs").textContent = String(--waiting);
      say(t(SAID[b.dataset.act]));
    });
  });

  // 4: a hit's own receipts
  var hits = Array.prototype.slice.call(document.querySelectorAll('input[name="hit"]'));
  function read(b, cast) {
    var r = Stage.receipts(b.value), name = b.nextElementSibling.textContent;
    b.checked = true;
    byId("hit-name").textContent = name;
    byId("hit-entries").textContent = and(r.entries);
    tier(byId("hit-trust"), r.confirmed, "trust");
    if (cast) {
      pick(4, "sel", b.value);
      on(4).choose("sel", b.value);
      say(t("hitSaid", { name: name, entries: and(r.entries), tier: r.confirmed ? t("humanConfirmed") : t("agent") }));
    }
  }
  hits.forEach(function (b) { b.addEventListener("change", function () { read(b, true); }); });

  // 5: the verdict, or a bare claim, behind the ring
  var claims = Array.prototype.slice.call(document.querySelectorAll('input[name="claim"]'));
  function behind(bare, cast) {
    claims.forEach(function (c) { c.checked = (c.value === "bare") === bare; });
    tier(byId("claim-laptop"), !bare, bare ? "laptopBare" : "laptopVerdict");
    tier(byId("claim-desktop"), !bare, bare ? "desktopBare" : "desktopVerdict");
    if (cast) {
      pick(5, "bare", bare);
      on(5).choose("bare", bare);
      say(t(bare ? "saidBare" : "saidVerdict"));
    }
  }
  claims.forEach(function (b) { b.addEventListener("change", function () { behind(b.value === "bare", true); }); });

  // a replay starts its scene over, and its step's words with it
  function reset(i) {
    delete picked[i];
    if (i === 2) replayed(6);
    if (i === 3) {
      waiting = 3;
      byId("needs").textContent = "3";
      acts.forEach(function (b) {
        b.removeAttribute("aria-disabled");
        b.textContent = t("accept");
        byId("row-" + b.dataset.act).textContent = byId("row-" + b.dataset.act).dataset.was;
      });
    }
    if (i === 4) read(hits[0], false);
    if (i === 5) behind(false, false);
  }
})();
