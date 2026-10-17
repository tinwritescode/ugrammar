import { describe, expect, test } from "bun:test";
import { FixFlow, type FlowDeps, type Status } from "../src/main/flow";
import { EngineError } from "../src/main/engine/types";
import { DEFAULT_SETTINGS } from "../src/main/settings";

function setup(over: Partial<FlowDeps> = {}, paused = false) {
  const log = { status: [] as Status[], notes: [] as string[], msgs: [] as string[], writes: [] as string[], engineCalls: 0 };
  const deps: FlowDeps = {
    settings: () => ({ ...DEFAULT_SETTINGS, paused, dictionary: ["Xpiki"] }),
    readSelection: async () => ({ ok: true, text: "I has a apple.", method: "ax", pid: 7, appName: "Notes" }),
    writeSelection: async (t) => (log.writes.push(t), { ok: true }),
    makeEngine: async () => ({
      check: async (r) => (log.engineCalls++, { original: r.text, corrected: "I have an apple." }),
      testConnection: async () => ({ ok: true, message: "" }),
    }),
    setStatus: (s) => log.status.push(s),
    notify: (m) => log.notes.push(m),
    showDiff: async () => "accept",
    showMessage: (m) => log.msgs.push(m),
    ...over,
  };
  return { flow: new FixFlow(deps), log };
}

describe("FixFlow", () => {
  test("accept writes corrected text", async () => {
    const { flow, log } = setup();
    expect(await flow.run()).toBe("applied");
    expect(log.writes).toEqual(["I have an apple."]);
  });
  test("dismiss never writes", async () => {
    const { flow, log } = setup({ showDiff: async () => "dismiss" });
    expect(await flow.run()).toBe("dismissed");
    expect(log.writes).toEqual([]);
  });
  test("paused does nothing", async () => {
    const { flow, log } = setup({}, true);
    expect(await flow.run()).toBe("paused");
    expect(log.engineCalls).toBe(0);
  });
  test("engine error: no write, error status + notification", async () => {
    const { flow, log } = setup({
      makeEngine: async () => ({
        check: async () => { throw new EngineError("timeout", "Timed out after 10s"); },
        testConnection: async () => ({ ok: true, message: "" }),
      }),
    });
    expect(await flow.run()).toBe("error");
    expect(log.writes).toEqual([]);
    expect(log.status.at(-1)?.kind).toBe("error");
    expect(log.notes[0]).toContain("not changed");
  });
  test("secure field skipped before engine", async () => {
    const { flow, log } = setup({ readSelection: async () => ({ ok: false, error: "secure_field" }) });
    expect(await flow.run()).toBe("skipped");
    expect(log.engineCalls).toBe(0);
    expect(log.msgs[0]).toContain("secure");
  });
  test("excluded apps list passed to helper", async () => {
    let seen: string[] = [];
    const { flow } = setup({ readSelection: async (ex) => ((seen = ex), { ok: false, error: "excluded_app" }) });
    await flow.run();
    expect(seen).toContain("com.apple.Terminal");
  });
  test("needs permission", async () => {
    const { flow, log } = setup({ readSelection: async () => ({ ok: false, error: "accessibility_not_trusted" }) });
    expect(await flow.run()).toBe("needs_permission");
    expect(log.status.at(-1)?.kind).toBe("needs_permission");
  });
  test("unchanged text shows message, no popover/write", async () => {
    const { flow, log } = setup({
      makeEngine: async () => ({ check: async (r) => ({ original: r.text, corrected: r.text }), testConnection: async () => ({ ok: true, message: "" }) }),
      showDiff: async () => { throw new Error("should not show"); },
    });
    expect(await flow.run()).toBe("unchanged");
    expect(log.writes).toEqual([]);
  });
  test("write failure notifies", async () => {
    const { flow, log } = setup({ writeSelection: async () => ({ ok: false, error: "focus_changed" }) });
    expect(await flow.run()).toBe("write_failed");
    expect(log.notes[0]).toContain("Focus changed");
  });
  test("second press while busy ignored", async () => {
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    const { flow } = setup({ readSelection: async () => (await gate, { ok: false, error: "no_selection" }) });
    const first = flow.run();
    expect(await flow.run()).toBe("busy");
    release();
    await first;
  });
});
