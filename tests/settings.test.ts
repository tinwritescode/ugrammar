import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { mkdtempSync, readFileSync, rmSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DEFAULT_SETTINGS, SettingsStore, normalizeSettings } from "../src/main/settings";

let dir: string;
beforeEach(() => (dir = mkdtempSync(join(tmpdir(), "proofy-"))));
afterEach(() => rmSync(dir, { recursive: true, force: true }));

describe("SettingsStore", () => {
  test("defaults when no file", () => {
    const s = new SettingsStore(dir);
    expect(s.get().endpoint).toBe("http://localhost:20128/v1");
    expect(s.get().model).toBe("cx/gpt-6-luna");
    expect(s.get().hotkey).toEqual({ key: "G", mods: ["alt", "cmd"] });
  });
  test("persists to settings.json and reloads", () => {
    const s = new SettingsStore(dir);
    s.update({ endpoint: "http://x/v1", model: "m2" });
    expect(existsSync(join(dir, "settings.json"))).toBe(true);
    const s2 = new SettingsStore(dir);
    expect(s2.get().endpoint).toBe("http://x/v1");
    expect(s2.get().model).toBe("m2");
  });
  test("never stores api key in JSON", () => {
    const s = new SettingsStore(dir);
    s.update({ endpoint: "http://x/v1", apiKey: "secret" } as never);
    expect(readFileSync(join(dir, "settings.json"), "utf8")).not.toContain("secret");
  });
  test("corrupt file falls back to defaults", () => {
    writeFileSync(join(dir, "settings.json"), "{not json");
    expect(new SettingsStore(dir).get()).toEqual(DEFAULT_SETTINGS);
  });
  test("dictionary add dedupes case-insensitively, trims, removes", () => {
    const s = new SettingsStore(dir);
    s.addWord(" Xpiki ");
    s.addWord("xpiki");
    s.addWord("");
    expect(s.get().dictionary).toEqual(["Xpiki"]);
    s.removeWord("XPIKI");
    expect(s.get().dictionary).toEqual([]);
  });
  test("excluded apps add/remove", () => {
    const s = new SettingsStore(dir);
    const base = s.get().excludedApps.length;
    s.addExcludedApp("com.foo.bar");
    s.addExcludedApp("com.foo.bar");
    expect(s.get().excludedApps.length).toBe(base + 1);
    s.removeExcludedApp("com.foo.bar");
    expect(s.get().excludedApps.length).toBe(base);
  });
  test("defaults exclude password managers and terminals", () => {
    const ex = DEFAULT_SETTINGS.excludedApps;
    expect(ex).toContain("com.apple.Terminal");
    expect(ex).toContain("com.1password.1password");
  });
  test("notifies listeners on change", () => {
    const s = new SettingsStore(dir);
    let n = 0;
    s.onChange(() => n++);
    s.update({ model: "z" });
    expect(n).toBe(1);
  });
});

describe("normalizeSettings", () => {
  test("rejects bad types, keeps valid fields, strips trailing slash", () => {
    const n = normalizeSettings({ endpoint: "http://a/v1/", model: 5, dictionary: ["a", 3, "a"], paused: "yes", timeoutMs: -1 });
    expect(n.endpoint).toBe("http://a/v1");
    expect(n.model).toBe(DEFAULT_SETTINGS.model);
    expect(n.dictionary).toEqual(["a"]);
    expect(n.paused).toBe(false);
    expect(n.timeoutMs).toBe(10000);
  });
  test("invalid hotkey falls back", () => {
    expect(normalizeSettings({ hotkey: { key: "??", mods: [] } }).hotkey).toEqual(DEFAULT_SETTINGS.hotkey);
  });
  test("showBadge defaults off, only true enables", () => {
    expect(DEFAULT_SETTINGS.showBadge).toBe(false);
    expect(normalizeSettings({}).showBadge).toBe(false);
    expect(normalizeSettings({ showBadge: "yes" }).showBadge).toBe(false);
    expect(normalizeSettings({ showBadge: true }).showBadge).toBe(true);
  });
});
