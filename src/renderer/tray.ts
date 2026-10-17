import { proofy } from "./bridge";
import type { Hotkey, Settings, TrayState } from "../shared/ipc";

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const panel = $("panel");

const STATUS_TEXT: Record<TrayState["status"], string> = {
  idle: "Sẵn sàng", checking: "Đang kiểm tra…", paused: "Đã tạm dừng", error: "Có lỗi", needs_permission: "Cần cấp quyền",
};

let state: TrayState | null = null;

const LANGS = ["Vietnamese", "English", "Spanish", "French", "German", "Portuguese", "Japanese", "Korean", "Chinese"];
const langSel = $<HTMLSelectElement>("explainLang");
for (const l of LANGS) langSel.add(new Option(l, l));
const keySel = $<HTMLSelectElement>("hkKey");
for (const c of "ABCDEFGHIJKLMNOPQRSTUVWXYZ") keySel.add(new Option(c, c));

const save = (patch: Partial<Settings>) => void proofy.saveSettings(patch);

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls: string, ...kids: (Node | string)[]) {
  const e = document.createElement(tag);
  e.className = cls;
  e.append(...kids);
  return e;
}

function host(endpoint: string) {
  try { return new URL(endpoint).host; } catch { return endpoint; }
}

/** Live updates must not clobber a field the user is typing in. */
function setValue(id: string, v: string) {
  const i = $<HTMLInputElement | HTMLSelectElement>(id);
  if (document.activeElement !== i) i.value = v;
}

function renderTags(id: string, items: string[], onRemove: (v: string) => void) {
  $(id).replaceChildren(...items.map((t) => {
    const x = el("button", "", "×");
    x.title = "Xoá";
    x.onclick = () => onRemove(t);
    return el("span", "tag", el("span", "", t), x);
  }));
}

function render(s: TrayState) {
  state = s;
  const cfg = s.settings;
  panel.dataset.status = s.status;
  panel.dataset.paused = String(cfg.paused);
  $("statusText").textContent = s.message && s.status === "error" ? `${STATUS_TEXT.error} — ${s.message}` : STATUS_TEXT[s.status];
  $("heroTitle").textContent = cfg.paused ? "Đang nghỉ" : "Đang canh chính tả";
  $("hotkey").textContent = s.hotkey;
  $("perm").hidden = s.status !== "needs_permission";
  const mode = cfg.autoCheck ? cfg.autoCheckMode : "off";
  for (const b of $("mode").querySelectorAll<HTMLButtonElement>("button")) b.classList.toggle("on", b.dataset.mode === mode);
  for (const c of document.querySelectorAll<HTMLInputElement>(".switch")) c.checked = cfg[c.dataset.key as "quickFix" | "explain" | "showBadge"];
  $("insights").replaceChildren(...(s.insights.length
    ? s.insights.map((i) => el("li", "", el("span", "pair", el("del", "", i.from || "∅"), " → ", el("ins", "", i.to || "∅")), el("span", "count", `${i.count}×`)))
    : [el("li", "empty", "Chưa có lỗi lặp lại. Tốt lắm!")]));
  $("engine").textContent = `${cfg.model} · ${host(cfg.endpoint)}`;
  $("engine").title = cfg.endpoint;
  if (![...langSel.options].some((o) => o.value === cfg.explainLang)) langSel.add(new Option(cfg.explainLang, cfg.explainLang));
  setValue("explainLang", cfg.explainLang);
  setValue("endpoint", cfg.endpoint);
  setValue("model", cfg.model);
  $("keyState").textContent = s.hasApiKey ? "· đã lưu trong Keychain" : "";
  for (const b of $("mods").querySelectorAll<HTMLButtonElement>("button")) b.classList.toggle("on", cfg.hotkey.mods.includes(b.dataset.mod as Hotkey["mods"][number]));
  keySel.value = cfg.hotkey.key;
  renderTags("words", cfg.dictionary, (w) => save({ dictionary: cfg.dictionary.filter((x) => x !== w) }));
  renderTags("apps", cfg.excludedApps, (a) => save({ excludedApps: cfg.excludedApps.filter((x) => x !== a) }));
  resize();
}

// Report the full content height; main caps it.
function resize() {
  requestAnimationFrame(() => proofy.trayResize(panel.scrollHeight + 2));
}

$("power").onclick = () => state && save({ paused: !state.settings.paused });
function showView(v: "main" | "setup") {
  if (panel.dataset.view === v) return;
  panel.dataset.view = v;
  $("title").textContent = v === "setup" ? "Cài đặt" : "ugrammar";
  $("toSetup").title = v === "setup" ? "Quay lại" : "Cài đặt";
  panel.scrollTop = 0;
  resize();
}
$("toSetup").onclick = () => showView(panel.dataset.view === "setup" ? "main" : "setup");
let scrollIdle = 0;
panel.addEventListener("scroll", () => {
  panel.classList.add("scrolling");
  clearTimeout(scrollIdle);
  scrollIdle = window.setTimeout(() => panel.classList.remove("scrolling"), 800);
});
$("openAX").onclick = () => proofy.trayAction("ax");
$("quit").onclick = () => proofy.trayAction("quit");
for (const b of $("mode").querySelectorAll<HTMLButtonElement>("button")) {
  b.onclick = () => save(b.dataset.mode === "off" ? { autoCheck: false } : { autoCheck: true, autoCheckMode: b.dataset.mode === "popup" ? "popup" : "underline" });
}
for (const b of document.querySelectorAll<HTMLButtonElement>("[data-tone]")) {
  b.onclick = () => proofy.trayAction(b.dataset.tone === "friendly" ? "friendly" : b.dataset.tone === "short" ? "short" : "formal");
}
for (const c of document.querySelectorAll<HTMLInputElement>(".switch")) {
  c.onchange = () => save({ [c.dataset.key as "quickFix" | "explain" | "showBadge"]: c.checked });
}

langSel.onchange = () => save({ explainLang: langSel.value });
for (const id of ["endpoint", "model"] as const) {
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
  out.className = "small mut";
  out.textContent = "Đang thử…";
  const r = await proofy.testConnection({
    endpoint: $<HTMLInputElement>("endpoint").value,
    model: $<HTMLInputElement>("model").value,
    apiKey: $<HTMLInputElement>("apiKey").value || undefined,
  });
  out.className = `small ${r.ok ? "ok" : "bad"}`;
  out.textContent = (r.ok ? "✓ " : "✕ ") + r.message;
  out.title = r.message;
};

function saveHotkey(mods: Hotkey["mods"]) {
  if (mods.length) save({ hotkey: { key: keySel.value, mods } }); // at least one modifier, else keep the old one
}
for (const b of $("mods").querySelectorAll<HTMLButtonElement>("button")) {
  b.onclick = () => {
    const m = b.dataset.mod as Hotkey["mods"][number];
    const cur = state?.settings.hotkey.mods ?? [];
    saveHotkey(cur.includes(m) ? cur.filter((x) => x !== m) : [...cur, m]);
  };
}
keySel.onchange = () => saveHotkey(state?.settings.hotkey.mods ?? []);

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
addOnEnter("newApp", (a) => state && !state.settings.excludedApps.includes(a) && save({ excludedApps: [...state.settings.excludedApps, a] }));
$("pickApp").onclick = async () => {
  const id = await proofy.pickApp();
  if (id && state && !state.settings.excludedApps.includes(id)) save({ excludedApps: [...state.settings.excludedApps, id] });
};
$("clearInsights").onclick = () => void proofy.clearInsights();

proofy.onTray(render);
// Replay the drop-in each time the panel opens, not on live updates.
window.addEventListener("focus", () => { panel.style.animation = "none"; void panel.offsetWidth; panel.style.animation = ""; });
// Each open starts on the main view; not on blur, which a native dialog over the panel also fires.
document.addEventListener("visibilitychange", () => document.hidden && showView("main"));
proofy.getTray().then(render);
