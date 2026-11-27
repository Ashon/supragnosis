"use strict";
// --- Words (docs/i18n.md Sections 3 and 4) ---------------------------------------------------------
// The viewer's words, each entry a key and its English and Korean, so a missing or stale translation
// sits next to its English. A sentence is one entry, looked up with word(key, values) and its {name}
// placeholders filled; an English entry may hold a one and an other form, picked by the count `n`.
// A name, a type, an id and what an agent wrote stay as written.
const WORDS = {
  // the page's markup (viewer.html), filled below before the rest of the script reads it
  pageTitle: { en: "Graph - Supragnosis", ko: "그래프 - Supragnosis" },
  mapLabel: { en: "Map of the entities and relations in this scope", ko: "이 범위의 개체와 관계 지도" },
  entities: { en: "Entities", ko: "개체" },
  emptyTitle: { en: "Nothing here yet.", ko: "아직 아무것도 없습니다." },
  emptySub: { en: "Ask a connected AI app to remember something, or pick a workspace that holds knowledge.", ko: "연결된 AI 앱에 무언가를 기억해 달라고 하거나, 지식이 있는 워크스페이스를 고르십시오." },
  arranging: { en: "Arranging...", ko: "배치하는 중..." },
  filtersPanel: { en: "Filters panel", ko: "필터 패널" },
  graph: { en: "Graph", ko: "그래프" },
  backToGraph: { en: "Back to the graph", ko: "그래프로 돌아가기" },
  allWorkspaces: { en: "All workspaces", ko: "모든 워크스페이스" },
  searchPlaceholder: { en: "search knowledge", ko: "지식 검색" },
  search: { en: "Search", ko: "검색" },
  searchResults: { en: "search results", ko: "검색 결과" },
  logPanel: { en: "Log panel", ko: "로그 패널" },
  activity: { en: "Activity", ko: "활동" },
  openLog: { en: "Open the Log", ko: "로그 열기" },
  entityTypes: { en: "Entity types", ko: "개체 타입" },
  relationTypes: { en: "Relation types", ko: "관계 타입" },
  definitions: { en: "Definitions", ko: "정의" },
  trust: { en: "Trust", ko: "신뢰" },
  said: { en: "Said", ko: "언급" },
  onTheMap: { en: "On the map", ko: "지도 범례" },
  log: { en: "Log", ko: "로그" },
  review: { en: "Review", ko: "검토" },
  backToReview: { en: "Back to Review", ko: "검토로 돌아가기" },
  done: { en: "Done", ko: "완료" },
  showAll: { en: "Show all", ko: "모두 보기" },
  clear: { en: "Clear", ko: "지우기" },
  retry: { en: "Retry", ko: "다시 시도" },
  openReview: { en: "Open the Review page", ko: "검토 페이지 열기" },
  syncSettings: { en: "Sync settings", ko: "동기화 설정" },
  settings: { en: "Settings", ko: "설정" },
  close: { en: "Close", ko: "닫기" },
  minimap: { en: "Minimap", ko: "미니맵" },
  view: { en: "View", ko: "보기" },
  regions: { en: "Regions", ko: "영역" },
  regionsAll: { en: "All", ko: "전체" },
  regionsSelected: { en: "Selected", ko: "선택 항목" },
  regionsOff: { en: "Off", ko: "끔" },
  boundaries: { en: "Boundaries", ko: "경계" },
  show: { en: "Show", ko: "표시" },
  labels: { en: "Labels", ko: "이름" },
  labelsSub: { en: "Names of entities and regions.", ko: "개체와 영역의 이름입니다." },
  relations: { en: "Relations", ko: "관계" },
  relationsSub: { en: "Lines between entities.", ko: "개체 사이의 선입니다." },
  arrows: { en: "Arrows", ko: "화살표" },
  arrowsSub: { en: "Which way each relation points. An active relation always shows it.", ko: "각 관계가 가리키는 방향입니다. 활성 관계에는 언제나 보입니다." },
  history: { en: "History", ko: "이력" },
  historySub: { en: "Relations that were superseded, drawn faint.", ko: "대체된 관계를 흐리게 그립니다." },
  minimapSub: { en: "The whole graph, while you are zoomed in; click or drag it to move.", ko: "확대해 있는 동안 그래프 전체를 보여 줍니다. 클릭하거나 끌어서 이동합니다." },
  arrangeByType: { en: "Arrange by type", ko: "타입별로 배치" },
  arrangeByTypeSub: { en: "A circle of entities for each type, with the relations between types kept.", ko: "타입마다 개체를 원으로 모으고, 타입 사이의 관계는 그대로 둡니다." },
  footprint: { en: "Footprint", ko: "발자국" },
  footprintSub: { en: "A glow on what agents used.", ko: "에이전트가 쓴 것을 빛으로 표시합니다." },
  pulses: { en: "Pulses", ko: "파동" },
  pulsesSub: { en: "A ring when an entity is touched.", ko: "개체를 건드리면 고리를 그립니다." },
  peers: { en: "Peers", ko: "피어" },
  peersSub: { en: "Other machines, as dots that move to the entities they touch. On by default on a hub.", ko: "다른 기기를 점으로 보여 주며, 점은 그 기기가 건드린 개체로 움직입니다. 허브에서는 기본으로 켜집니다." },
  more: { en: "More", ko: "더 보기" },
  resetLayout: { en: "Reset layout", ko: "배치 초기화" },
  resetLayoutSub: { en: "Forgets where this scope's entities were and lays it out again.", ko: "이 범위의 개체 위치를 잊고 다시 배치합니다." },
  keysMore: { en: "Keys...", ko: "단축키..." },
  shownWithRelations: { en: "Shown with relations.", ko: "관계와 함께 표시됩니다." },
  keys: { en: "Keys", ko: "단축키" },
  find: { en: "Find", ko: "찾기" },
  panels: { en: "Panels", ko: "패널" },
  map: { en: "Map", ko: "지도" },
  selection: { en: "Selection", ko: "선택" },
  lists: { en: "Lists", ko: "목록" },
  or: { en: "or", ko: "또는" },
  and: { en: "and", ko: "및" },
  keepAnswer: { en: "Keep the answer on the map", ko: "검색 결과를 지도에 남기기" },
  filters: { en: "Filters", ko: "필터" },
  zoomIn: { en: "Zoom in", ko: "확대" },
  zoomOut: { en: "Zoom out", ko: "축소" },
  fitToView: { en: "Fit to view", ko: "화면에 맞추기" },
  mapOrList: { en: "Map or List", ko: "지도 또는 목록" },
  backAndForward: { en: "Back and Forward", ko: "뒤로, 앞으로" },
  closeTop: { en: "Close what is on top", ko: "맨 위의 것 닫기" },
  arrowKeys: { en: "Arrow keys", ko: "화살표 키" },
  move: { en: "Move", ko: "이동" },
  choose: { en: "Choose", ko: "고르기" },
  singleKeys: { en: "Single-key shortcuts", ko: "단일 키 단축키" },
  singleKeysSub: { en: "Letters and symbols act on the Graph page, outside a text field, while no dialog or card is open.", ko: "글자와 기호 키는 그래프 페이지에서, 텍스트 필드 밖에서, 대화상자나 카드가 열려 있지 않을 때 동작합니다." },
  sameAs: { en: "Same as...", ko: "같은 개체..." },
  rejectAllBlocked: { en: "Reject all blocked", ko: "막힌 제안 모두 거절" },
  blockedWhy: { en: "If a check passes later, the accept it carries still merges it.", ko: "나중에 점검을 통과하면, 제안에 담긴 수락이 그대로 병합합니다." },
  cancel: { en: "Cancel", ko: "취소" },
  rejectAll: { en: "Reject all", ko: "모두 거절" },
  follow: { en: "Follow", ko: "따라가기" },
  showAs: { en: "Show as", ko: "보기 방식" },
  list: { en: "List", ko: "목록" },
  listIdle: { en: "List shows the map as a table.", ko: "목록은 지도를 표로 보여 줍니다." },
  viewOptions: { en: "View options", ko: "보기 옵션" },
};
// The language: the app's, which the desktop shell hands every page it opens (shell-init.js), or in
// a browser the first of its languages that is Korean or English, and English when neither is
// listed (Section 3). The page says which on its root.
const LANG = (() => {
  const shell = window.supragnosisWords && window.supragnosisWords.lang;
  if (shell === "en" || shell === "ko") return shell;
  const nav = window.navigator;
  for (const l of nav.languages || [nav.language || ""]) {
    const base = String(l).toLowerCase().split("-")[0];
    if (base === "ko" || base === "en") return base;
  }
  return "en";
})();
document.documentElement.lang = LANG;
// A sentence in the page's language, its {name} placeholders filled, a number in the language's
// form. A count picks between two entries, one for one and one for more, as the site's pages do: an
// entry is one string in each language, and Korean says both alike.
const NUMBER = new Intl.NumberFormat(LANG);
function word(key, values) {
  const e = WORDS[key];
  if (!e) return key;   // a key the words lack, which the words test rules out
  return e[LANG].replace(/\{(\w+)\}/g, (m, k) => {
    if (!values || !(k in values)) return m;
    return String(typeof values[k] === "number" ? NUMBER.format(values[k]) : values[k]);
  });
}
// The markup holds its English and marks each string with its key: data-t for its text (an icon, a
// key or a count beside it stays), data-t-label for its aria-label and title (a title keeps the key
// it names, "(Esc)"), data-t-ph for a field's placeholder.
for (const el of document.querySelectorAll("[data-t]")) {
  const text = [...el.childNodes].find(n => n.nodeType === 3 && n.textContent.trim());   // a text node
  const said = word(el.dataset.t);
  if (text) text.textContent = text.textContent.replace(/\S(?:[\s\S]*\S)?/, () => said);
  else el.textContent = said;
}
for (const el of document.querySelectorAll("[data-t-label]")) {
  const name = word(el.dataset.tLabel);
  if (el.hasAttribute("aria-label")) el.setAttribute("aria-label", name);
  if (el.hasAttribute("title")) {
    const key = / \(([^)]+)\)$/.exec(el.title);
    el.title = tipText(name, key && key[1]);
  }
}
for (const el of document.querySelectorAll("[data-t-ph]")) el.placeholder = word(el.dataset.tPh);

