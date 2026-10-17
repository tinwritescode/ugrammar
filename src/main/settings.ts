import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { join } from "node:path";

export interface Hotkey {
  key: string; // A-Z
  mods: ("cmd" | "alt" | "ctrl" | "shift")[];
}

export interface Settings {
  endpoint: string;
  model: string;
  timeoutMs: number;
  dictionary: string[];
  excludedApps: string[]; // bundle ids
  hotkey: Hotkey;
  paused: boolean;
  firstRunAcknowledged: boolean;
}

export const DEFAULT_SETTINGS: Settings = {
  endpoint: "http://localhost:20128/v1",
  model: "cx/gpt-6-luna",
  timeoutMs: 10000,
  dictionary: [],
  excludedApps: [
    "com.1password.1password",
    "com.agilebits.onepassword7",
    "com.bitwarden.desktop",
    "com.apple.keychainaccess",
    "com.apple.Passwords",
    "com.apple.Terminal",
    "com.googlecode.iterm2",
    "com.mitchellh.ghostty",
    "dev.warp.Warp-Stable",
  ],
  hotkey: { key: "G", mods: ["alt", "cmd"] },
  paused: false,
  firstRunAcknowledged: false,
};

const MODS = ["cmd", "alt", "ctrl", "shift"] as const;

const strList = (v: unknown, fallback: string[]) =>
  Array.isArray(v) ? dedupe(v.filter((x): x is string => typeof x === "string").map((x) => x.trim()).filter(Boolean)) : fallback;

function dedupe(list: string[]): string[] {
  const seen = new Set<string>();
  return list.filter((w) => {
    const k = w.toLowerCase();
    if (seen.has(k)) return false;
    seen.add(k);
    return true;
  });
}

export function normalizeSettings(raw: unknown): Settings {
  const r = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  const d = DEFAULT_SETTINGS;
  const str = (v: unknown, f: string) => (typeof v === "string" && v.trim() ? v.trim() : f);
  const hk = r.hotkey as Partial<Hotkey> | undefined;
  const hotkeyOk =
    hk && typeof hk.key === "string" && /^[A-Za-z]$/.test(hk.key) && Array.isArray(hk.mods) &&
    hk.mods.length > 0 && hk.mods.every((m) => (MODS as readonly string[]).includes(m));
  return {
    endpoint: str(r.endpoint, d.endpoint).replace(/\/+$/, ""),
    model: str(r.model, d.model),
    timeoutMs: typeof r.timeoutMs === "number" && r.timeoutMs >= 1000 && r.timeoutMs <= 120000 ? r.timeoutMs : d.timeoutMs,
    dictionary: strList(r.dictionary, []),
    excludedApps: strList(r.excludedApps, [...d.excludedApps]),
    hotkey: hotkeyOk ? { key: hk!.key!.toUpperCase(), mods: [...hk!.mods!] as Hotkey["mods"] } : { ...d.hotkey, mods: [...d.hotkey.mods] },
    paused: r.paused === true,
    firstRunAcknowledged: r.firstRunAcknowledged === true,
  };
}

/** Persists to <dir>/settings.json. The API key is never part of Settings (it lives in the Keychain). */
export class SettingsStore {
  private data: Settings;
  private listeners: ((s: Settings) => void)[] = [];
  private file: string;

  constructor(private dir: string) {
    this.file = join(dir, "settings.json");
    this.data = this.load();
  }

  private load(): Settings {
    try {
      if (existsSync(this.file)) return normalizeSettings(JSON.parse(readFileSync(this.file, "utf8")));
    } catch {}
    return normalizeSettings({});
  }

  get(): Settings {
    return structuredClone(this.data);
  }

  update(patch: Partial<Settings>): Settings {
    this.data = normalizeSettings({ ...this.data, ...patch });
    mkdirSync(this.dir, { recursive: true });
    const tmp = this.file + ".tmp";
    writeFileSync(tmp, JSON.stringify(this.data, null, 2) + "\n");
    renameSync(tmp, this.file);
    for (const l of this.listeners) l(this.get());
    return this.get();
  }

  onChange(fn: (s: Settings) => void) {
    this.listeners.push(fn);
  }

  addWord(word: string) {
    this.update({ dictionary: [...this.data.dictionary, word] });
  }
  removeWord(word: string) {
    this.update({ dictionary: this.data.dictionary.filter((w) => w.toLowerCase() !== word.trim().toLowerCase()) });
  }
  addExcludedApp(id: string) {
    this.update({ excludedApps: [...this.data.excludedApps, id] });
  }
  removeExcludedApp(id: string) {
    this.update({ excludedApps: this.data.excludedApps.filter((x) => x !== id.trim()) });
  }
}
