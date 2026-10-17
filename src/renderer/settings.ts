import type { ProofyApi, SettingsView } from "../shared/ipc";
import type { Settings } from "../main/settings";
declare const proofy: ProofyApi;

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
let dictionary: string[] = [];
let excluded: string[] = [];
let mods = new Set<string>();

const MOD_LABELS: [string, string][] = [["ctrl", "⌃ Control"], ["alt", "⌥ Option"], ["shift", "⇧ Shift"], ["cmd", "⌘ Command"]];

function renderList(ul: HTMLElement, items: string[], onRemove: (i: number) => void) {
  ul.replaceChildren(...items.map((t, i) => {
    const li = document.createElement("li");
    const span = document.createElement("span");
    span.textContent = t;
    const b = document.createElement("button");
    b.textContent = "×";
    b.onclick = () => onRemove(i);
    li.append(span, b);
    return li;
  }));
}

function renderLists() {
  renderList($("words"), dictionary, (i) => { dictionary.splice(i, 1); renderLists(); });
  renderList($("apps"), excluded, (i) => { excluded.splice(i, 1); renderLists(); });
}

function renderMods() {
  $("mods").replaceChildren(...MOD_LABELS.map(([id, label]) => {
    const l = document.createElement("label");
    l.style.margin = "0";
    const c = document.createElement("input");
    c.type = "checkbox";
    c.checked = mods.has(id);
    c.style.display = "inline";
    c.style.width = "auto";
    c.onchange = () => { c.checked ? mods.add(id) : mods.delete(id); };
    l.append(c, " " + label);
    return l;
  }));
}

function load(v: SettingsView) {
  const s = v.settings;
  $<HTMLInputElement>("endpoint").value = s.endpoint;
  $<HTMLInputElement>("model").value = s.model;
  $("keyState").textContent = v.hasApiKey ? "(stored in Keychain)" : "(none)";
  $<HTMLInputElement>("apiKey").value = "";
  $("perm").hidden = v.axTrusted;
  dictionary = [...s.dictionary];
  excluded = [...s.excludedApps];
  $<HTMLInputElement>("showBadge").checked = s.showBadge;
  $<HTMLInputElement>("autoCheck").checked = s.autoCheck;
  $<HTMLSelectElement>("autoCheckMode").value = s.autoCheckMode;
  $<HTMLInputElement>("quickFix").checked = s.quickFix;
  $<HTMLInputElement>("explain").checked = s.explain;
  if (![...langSel.options].some((o) => o.value === s.explainLang)) langSel.add(new Option(s.explainLang, s.explainLang));
  langSel.value = s.explainLang;
  $("insights").replaceChildren(...(v.insights.length ? v.insights : []).map((i) => {
    const li = document.createElement("li");
    const span = document.createElement("span");
    span.textContent = `${i.count}× ${i.note ?? `${i.from} → ${i.to}`}`;
    li.append(span);
    return li;
  }));
  if (!v.insights.length) { const li = document.createElement("li"); li.textContent = "Nothing repeated yet."; $("insights").append(li); }
  mods = new Set(s.hotkey.mods);
  $<HTMLSelectElement>("hkKey").value = s.hotkey.key;
  renderMods();
  renderLists();
}

const langSel = $<HTMLSelectElement>("explainLang");
for (const l of ["Vietnamese", "English", "Spanish", "French", "German", "Portuguese", "Japanese", "Korean", "Chinese"]) langSel.add(new Option(l, l));
$("clearInsights").onclick = async () => load(await proofy.clearInsights());
const sel = $<HTMLSelectElement>("hkKey");
for (const c of "ABCDEFGHIJKLMNOPQRSTUVWXYZ") sel.add(new Option(c, c));

function addTo(list: string[], input: HTMLInputElement) {
  const v = input.value.trim();
  if (v && !list.some((x) => x.toLowerCase() === v.toLowerCase())) list.push(v);
  input.value = "";
  renderLists();
}

$("addWord").onclick = () => addTo(dictionary, $<HTMLInputElement>("newWord"));
$("newWord").onkeydown = (e) => { if ((e as KeyboardEvent).key === "Enter") addTo(dictionary, $<HTMLInputElement>("newWord")); };
$("addApp").onclick = () => addTo(excluded, $<HTMLInputElement>("newApp"));
$("pickApp").onclick = async () => {
  const id = await proofy.pickApp();
  if (id && !excluded.includes(id)) { excluded.push(id); renderLists(); }
};
$("openAX").onclick = () => proofy.openAccessibilitySettings();

$("test").onclick = async () => {
  const out = $("testResult");
  out.className = "mut";
  out.textContent = "Testing…";
  const r = await proofy.testConnection({
    endpoint: $<HTMLInputElement>("endpoint").value,
    model: $<HTMLInputElement>("model").value,
    apiKey: $<HTMLInputElement>("apiKey").value || undefined,
  });
  out.className = r.ok ? "ok" : "bad";
  out.textContent = (r.ok ? "✓ " : "✕ ") + r.message;
};

$("save").onclick = async () => {
  const key = $<HTMLInputElement>("apiKey").value;
  if (key) await proofy.setApiKey(key);
  const patch: Partial<Settings> = {
    endpoint: $<HTMLInputElement>("endpoint").value,
    model: $<HTMLInputElement>("model").value,
    showBadge: $<HTMLInputElement>("showBadge").checked,
    autoCheck: $<HTMLInputElement>("autoCheck").checked,
    autoCheckMode: $<HTMLSelectElement>("autoCheckMode").value === "popup" ? "popup" : "underline",
    quickFix: $<HTMLInputElement>("quickFix").checked,
    explain: $<HTMLInputElement>("explain").checked,
    explainLang: langSel.value,
    dictionary,
    excludedApps: excluded,
  };
  if (mods.size > 0) patch.hotkey = { key: sel.value, mods: [...mods] as Settings["hotkey"]["mods"] };
  load(await proofy.saveSettings(patch));
  const s = $("saved");
  s.textContent = mods.size > 0 ? "Saved ✓" : "Saved ✓ (hotkey needs at least one modifier; kept previous)";
  setTimeout(() => (s.textContent = ""), 2500);
};

proofy.getSettings().then(load);
