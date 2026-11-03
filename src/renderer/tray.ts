import { proofy } from "./bridge";
import { el } from "./dom";
import { diffNodes, fixCount, note } from "./fix";
import { dict, translatePage, type Dict } from "./i18n";
import { mascot, type Mood } from "./mascot";
import type { AppInfo, Hotkey, Lang, PendingFix, Provider, Settings, TrayState, UpdateNote } from "../shared/ipc";

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const panel = $("panel");

const MOOD: Record<TrayState["status"], Mood> = {
  idle: "reading", checking: "checking", paused: "asleep", error: "confused", needs_permission: "key",
};

let state: TrayState | null = null;
let shownMood: Mood | null = null;
let t: Dict = dict(null);
let shownLang: Lang | null = null;

/** The owl card at the top of the main view. The mascot is only swapped when its mood changes, so its animation keeps running. */
function renderHero(s: TrayState) {
  const mood = MOOD[s.status];
  if (mood !== shownMood) {
    $("mascot").replaceChildren(mascot(mood, 80));
    shownMood = mood;
  }
  $("heroTitle").textContent = t.status[s.status];
  const note = s.status === "error" ? s.message ?? "" : s.status === "paused" ? t.asleepNote : s.status === "checking" ? t.readingNote : "";
  $("heroHint").hidden = s.status !== "idle";
  $("heroNote").hidden = !note;
  $("heroNote").textContent = note;
}

const langSel = $<HTMLSelectElement>("explainLang");
const uiLangSel = $<HTMLSelectElement>("language");
const LANGS: Record<Lang, true> = { en: true, vi: true };
const isLang = (v: string): v is Lang => Object.hasOwn(LANGS, v);

/** Re-labels the page when the picked language changes; the explain-language options are rebuilt in it. */
function setLanguage(lang: Lang | null) {
  if (shownLang === lang && langSel.options.length) return;
  shownLang = lang;
  t = dict(lang);
  translatePage(lang ?? "en", t);
  langSel.replaceChildren(...Object.entries(t.explainLangs).map(([v, label]) => new Option(label, v)));
  $("title").textContent = panel.dataset.view === "setup" ? t.html.settings : "ugrammar";
}
setLanguage(null);

const save = (patch: Partial<Settings>) => void proofy.saveSettings(patch);

type SwitchKey = "quickFix" | "explain";

const svg = (paths: string) => {
  const s = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  s.setAttribute("viewBox", "0 0 24 24");
  s.innerHTML = paths;
  return s;
};
const X_ICON = '<path d="M18 6 6 18"/><path d="m6 6 12 12"/>';
const MINUS_ICON = '<circle cx="12" cy="12" r="9"/><path d="M8 12h8"/>';

function host(endpoint: string) {
  try { return new URL(endpoint).host; } catch { return endpoint; }
}

const PROVIDERS: Record<Provider, true> = { openai: true, claudeCli: true };
const isProvider = (v: string): v is Provider => Object.hasOwn(PROVIDERS, v);
const providerSel = $<HTMLSelectElement>("provider");

/** Shows the fields of the engine picked in the select (it may not be saved yet). */
function showProviderFields() {
  for (const e of document.querySelectorAll<HTMLElement>("[data-provider]")) e.hidden = e.dataset.provider !== providerSel.value;
}

function engineLabel(cfg: Settings): { text: string; title: string } {
  switch (cfg.provider) {
    case "openai": return { text: `${cfg.model} · ${host(cfg.endpoint)}`, title: cfg.endpoint };
    case "claudeCli": return { text: `${cfg.claudeModel} · Claude CLI`, title: cfg.claudePath || "claude" };
    default: { const _exhaustive: never = cfg.provider; return _exhaustive; }
  }
}

/** Live updates must not clobber a field the user is typing in. */
function setValue(id: string, v: string) {
  const i = $<HTMLInputElement | HTMLSelectElement>(id);
  if (document.activeElement !== i) i.value = v;
}

/** "⌥⌘G" -> one keycap per symbol. */
const keycaps = (label: string) => [...label].map((k) => el("kbd", {}, k));

function renderTags(id: string, items: string[], onRemove: (v: string) => void) {
  $(id).replaceChildren(...items.map((w) =>
    el("span", { className: "tag" }, el("span", {}, w), el("button", { title: t.remove, ariaLabel: t.removeWord(w), onclick: () => onRemove(w) }, svg(X_ICON)))));
}

// ---------- Excluded apps ----------

const APPS_COLLAPSED = 5;
const appInfo = new Map<string, AppInfo>();
const appInfoAsked = new Set<string>();
let showAllApps = false;

function appIcon(info: AppInfo | undefined, id: string) {
  if (info?.icon) return el("img", { className: "icon", src: `data:image/png;base64,${info.icon}`, alt: "" });
  return el("span", { className: "icon initial", ariaHidden: "true" }, (info?.name ?? id.split(".").pop() ?? "?").charAt(0).toUpperCase());
}

function renderApps(ids: string[]) {
  const missing = ids.filter((id) => !appInfoAsked.has(id));
  if (missing.length) {
    for (const id of missing) appInfoAsked.add(id);
    void proofy.appInfo(missing).then((infos) => {
      for (const i of infos) appInfo.set(i.id, i);
      if (state) renderApps(state.settings.excludedApps);
    });
  }
  const shown = showAllApps ? ids : ids.slice(0, APPS_COLLAPSED);
  $("apps").replaceChildren(...shown.map((id) => {
    const info = appInfo.get(id);
    const name = el("span", { className: "name grow" }, el("span", {}, info?.name ?? id));
    if (info?.name) name.append(el("span", {}, id));
    const remove = el("button", { title: t.removeApp, ariaLabel: t.removeAppLabel(info?.name ?? id), onclick: () => state && save({ excludedApps: state.settings.excludedApps.filter((x) => x !== id) }) }, svg(MINUS_ICON));
    return el("li", { title: id }, appIcon(info, id), name, remove);
  }));
  const more = $("moreApps");
  more.hidden = ids.length <= APPS_COLLAPSED;
  more.textContent = showAllApps ? t.showLess : t.showMore(ids.length - APPS_COLLAPSED);
}

$("moreApps").onclick = () => {
  showAllApps = !showAllApps;
  if (state) renderApps(state.settings.excludedApps);
  resize();
};

// ---------- Pending fixes ----------

let openFix: number | null = null;

function renderPending(items: PendingFix[]) {
  $("pendingSection").hidden = !items.length;
  $("pendingCount").textContent = t.sentences(items.length);
  $("pending").replaceChildren(...items.map((f) => {
    const expanded = f.id === openFix;
    const original = f.ops.filter((op) => op.type !== "insert").map((op) => op.text).join("");
    const toggle = () => {
      openFix = expanded ? null : f.id;
      renderPending(items);
      resize();
    };
    const head = el("button", { className: "ghost head", ariaExpanded: String(expanded), onclick: toggle },
      el("span", { className: "grow" }, el("strong", {}, f.appName || "App"), el("span", { className: "snip" }, original)),
      el("span", { className: "count" }, t.fixes(fixCount(f.ops))));
    const li = el("li", { className: expanded ? "open" : "" }, head);
    if (expanded) {
      li.append(el("div", { className: "text" }, ...diffNodes(f.ops)));
      if (f.notes.length) li.append(el("ul", { className: "notes" }, ...f.notes.map(note)));
      li.append(el("div", { className: "bar" },
        el("button", { onclick: () => proofy.pendingAction(f.id, "dismiss") }, t.dismiss),
        el("button", { className: "primary", onclick: () => proofy.pendingAction(f.id, "accept") }, t.apply)));
    }
    return li;
  }));
}

// ---------- Updates ----------

function renderUpdateCheck(note?: UpdateNote) {
  const button = $<HTMLButtonElement>("checkUpdate");
  button.disabled = note === "checking" || note === "downloading";
  button.textContent = note === "checking" || note === "downloading" ? t.updateButton[note] : t.checkUpdates;
  $("updateNote").textContent = note ? t.updateNote[note] : t.autoUpdate;
}

// ---------- Hotkey recorder ----------

const MOD_ORDER: Hotkey["mods"] = ["ctrl", "alt", "shift", "cmd"];
let recording = false;

function renderRecorder() {
  const r = $("recorder");
  r.classList.toggle("rec", recording);
  $("recHint").hidden = !recording;
  r.replaceChildren(...(recording ? [t.recording] : keycaps(state?.hotkey ?? "")));
}

function setRecording(on: boolean) {
  if (recording === on) return;
  recording = on;
  renderRecorder();
  resize();
}

$("recorder").onclick = () => setRecording(!recording);
window.addEventListener("keydown", (e) => {
  if (!recording) return;
  e.preventDefault();
  if (e.key === "Escape") return setRecording(false);
  const key = /^Key([A-Z])$/.exec(e.code)?.[1];
  // Shift alone would steal capital letters from every app.
  if (!key || !(e.metaKey || e.altKey || e.ctrlKey)) return;
  const held = { ctrl: e.ctrlKey, alt: e.altKey, shift: e.shiftKey, cmd: e.metaKey };
  save({ hotkey: { key, mods: MOD_ORDER.filter((m) => held[m]) } });
  setRecording(false);
});
window.addEventListener("blur", () => setRecording(false));

// ---------- Render ----------

function render(s: TrayState) {
  state = s;
  const cfg = s.settings;
  setLanguage(cfg.language);
  panel.dataset.status = s.status;
  panel.dataset.paused = String(cfg.paused);
  $("statusText").textContent = s.message && s.status === "error" ? `${t.status.error} — ${s.message}` : t.status[s.status];
  $<HTMLInputElement>("power").checked = !cfg.paused;
  $("hotkey").replaceChildren(...keycaps(s.hotkey));
  renderHero(s);
  $("perm").hidden = s.status !== "needs_permission";
  $("update").hidden = !s.update;
  $("updateText").textContent = t.installed(s.update ?? "");
  renderUpdateCheck(s.updateNote);
  renderPending(s.pending);
  const mode = cfg.autoCheck ? "popup" : "off";
  for (const b of $("mode").querySelectorAll<HTMLButtonElement>("button")) b.classList.toggle("on", b.dataset.mode === mode);
  for (const c of document.querySelectorAll<HTMLInputElement>(".switch[data-key]")) c.checked = cfg[c.dataset.key as SwitchKey];
  $("insights").replaceChildren(...(s.insights.length
    ? s.insights.map((i) => el("li", { title: i.note ?? "" },
        el("span", { className: "pair" }, el("del", {}, i.from || "∅"), el("span", { className: "arrow" }, " → "), el("ins", {}, i.to || "∅")),
        el("span", { className: "count" }, t.times(i.count))))
    : [el("li", { className: "empty" }, t.noInsights)]));
  const engine = engineLabel(cfg);
  $("engine").textContent = engine.text;
  $("engine").title = engine.title;
  if (![...langSel.options].some((o) => o.value === cfg.explainLang)) langSel.add(new Option(cfg.explainLang, cfg.explainLang));
  setValue("explainLang", cfg.explainLang);
  setValue("language", cfg.language ?? "en");
  setValue("endpoint", cfg.endpoint);
  setValue("model", cfg.model);
  setValue("provider", cfg.provider);
  setValue("claudeModel", cfg.claudeModel);
  setValue("claudePath", cfg.claudePath);
  showProviderFields();
  $("keyState").textContent = s.hasApiKey ? t.keySaved : "";
  renderRecorder();
  renderTags("words", cfg.dictionary, (w) => save({ dictionary: cfg.dictionary.filter((x) => x !== w) }));
  renderApps(cfg.excludedApps);
  resize();
}

// Report the full content height; main caps it.
function resize() {
  requestAnimationFrame(() => proofy.trayResize(panel.scrollHeight));
}

function showView(v: "main" | "setup") {
  if (panel.dataset.view === v) return;
  panel.dataset.view = v;
  $("title").textContent = v === "setup" ? t.html.settings : "ugrammar";
  setRecording(false);
  panel.scrollTop = 0;
  resize();
}
$("toSetup").onclick = () => showView("setup");
$("back").onclick = () => showView("main");
$<HTMLInputElement>("power").onchange = (e) => save({ paused: !(e.target as HTMLInputElement).checked });

let scrollIdle = 0;
panel.addEventListener("scroll", () => {
  panel.classList.add("scrolling");
  clearTimeout(scrollIdle);
  scrollIdle = window.setTimeout(() => panel.classList.remove("scrolling"), 800);
});
$("openAX").onclick = () => proofy.trayAction("ax");
$("quit").onclick = () => proofy.trayAction("quit");
$("restart").onclick = () => proofy.trayAction("restart");
$("github").onclick = (e) => {
  e.preventDefault();
  proofy.trayAction("github");
};
$("license").onclick = (e) => {
  e.preventDefault();
  proofy.trayAction("license");
};
void proofy.appVersion().then((v) => ($("version").textContent = v));
$("checkUpdate").onclick = () => proofy.checkUpdate();
for (const b of $("mode").querySelectorAll<HTMLButtonElement>("button")) {
  b.onclick = () => save(b.dataset.mode === "off" ? { autoCheck: false } : { autoCheck: true });
}
for (const b of document.querySelectorAll<HTMLButtonElement>("[data-tone]")) {
  b.onclick = () => proofy.trayAction(b.dataset.tone === "friendly" ? "friendly" : b.dataset.tone === "short" ? "short" : "formal");
}
for (const c of document.querySelectorAll<HTMLInputElement>(".switch[data-key]")) {
  c.onchange = () => save({ [c.dataset.key as SwitchKey]: c.checked });
}

langSel.onchange = () => save({ explainLang: langSel.value });
uiLangSel.onchange = () => isLang(uiLangSel.value) && save({ language: uiLangSel.value });
providerSel.onchange = () => {
  showProviderFields();
  resize();
  if (isProvider(providerSel.value)) save({ provider: providerSel.value });
};
for (const id of ["endpoint", "model", "claudeModel", "claudePath"] as const) {
  $<HTMLInputElement>(id).onchange = (e) => save({ [id]: (e.target as HTMLInputElement).value });
}
$<HTMLInputElement>("apiKey").onchange = async (e) => {
  const i = e.target as HTMLInputElement;
  if (!i.value) return;
  await proofy.setApiKey(i.value);
  i.value = "";
};
$("test").onclick = async () => {
  const out = $("testResult");
  out.className = "pending";
  out.textContent = t.testing;
  const value = (id: string) => $<HTMLInputElement>(id).value;
  const r = await proofy.testConnection({
    settings: {
      provider: isProvider(providerSel.value) ? providerSel.value : "openai",
      endpoint: value("endpoint"),
      model: value("model"),
      claudeModel: value("claudeModel"),
      claudePath: value("claudePath"),
    },
    apiKey: value("apiKey") || undefined,
  });
  out.className = r.ok ? "ok" : "bad";
  out.textContent = (r.ok ? "✓ " : "✕ ") + r.message;
  out.title = r.message;
};

function addOnEnter(id: string, add: (v: string) => void) {
  const i = $<HTMLInputElement>(id);
  i.onkeydown = (e) => {
    const v = i.value.trim();
    if (e.key !== "Enter" || !v) return;
    add(v);
    i.value = "";
  };
}
const hasCI = (list: string[], v: string) => list.some((x) => x.toLowerCase() === v.toLowerCase());
addOnEnter("newWord", (w) => state && !hasCI(state.settings.dictionary, w) && save({ dictionary: [...state.settings.dictionary, w] }));
function excludeApp(id: string | null) {
  if (id && state && !state.settings.excludedApps.includes(id)) save({ excludedApps: [...state.settings.excludedApps, id] });
}
addOnEnter("newApp", excludeApp);
$("pickApp").onclick = async () => excludeApp(await proofy.pickApp());
$("clearInsights").onclick = () => void proofy.clearInsights();

proofy.onTray(render);
// Replay the drop-in each time the panel opens, not on live updates.
window.addEventListener("focus", () => { panel.style.animation = "none"; void panel.offsetWidth; panel.style.animation = ""; });
// Each open starts on the main view; not on blur, which a native dialog over the panel also fires.
document.addEventListener("visibilitychange", () => document.hidden && showView("main"));
proofy.getTray().then(render);
