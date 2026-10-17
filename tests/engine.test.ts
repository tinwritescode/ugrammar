import { describe, expect, test } from "bun:test";
import { OpenAICompatibleEngine, buildMessages, cleanModelOutput, parseNotes, postProcess } from "../src/main/engine/openai";
import { EngineError } from "../src/main/engine/types";
import { diffWords } from "../src/shared/diff";

const cfg = { endpoint: "http://localhost:20128/v1", model: "m", timeoutMs: 200 };

function fakeFetch(handler: (url: string, init: RequestInit) => Response | Promise<Response>) {
  return ((url: string, init: RequestInit) => Promise.resolve(handler(url, init))) as unknown as typeof fetch;
}
const ok = (content: string) =>
  new Response(JSON.stringify({ choices: [{ message: { content } }] }), { status: 200 });

describe("prompt", () => {
  test("injects dictionary words", () => {
    const m = buildMessages("hello", ["Xpiki", "Kubernetes"]);
    expect(m[0].content).toContain("Xpiki");
    expect(m[0].content).toContain("Kubernetes");
    expect(m[1].content).toContain("hello");
  });
  test("no dictionary section when empty", () => {
    expect(buildMessages("hi", [])[0].content).not.toContain("Do not change these words");
  });
});

describe("cleanModelOutput", () => {
  test("extracts <corrected> tags, ignoring chatter", () => {
    expect(cleanModelOutput("Sure! <corrected>I have an apple.</corrected> Done", "x")).toBe("I have an apple.");
  });
  test("keeps inner newlines", () => {
    expect(cleanModelOutput("<corrected>\nA.\nB.\n</corrected>", "x")).toBe("A.\nB.");
  });
  test("strips code fences and wrapping quotes", () => {
    expect(cleanModelOutput("```\nHello.\n```", "x")).toBe("Hello.");
    expect(cleanModelOutput('"Hello."', "x")).toBe("Hello.");
  });
  test("keeps quotes the original had", () => {
    expect(cleanModelOutput('"Hi"', '"Hi"')).toBe('"Hi"');
  });
});

describe("OpenAICompatibleEngine", () => {
  test("returns corrected text and hits /chat/completions with auth", async () => {
    let seen: { url: string; init: RequestInit } | undefined;
    const e = new OpenAICompatibleEngine(cfg, {
      apiKey: "k",
      fetch: fakeFetch((url, init) => ((seen = { url, init }), ok("I have an apple."))),
    });
    const r = await e.check({ text: "I has a apple.", dictionary: [] });
    expect(r.corrected).toBe("I have an apple.");
    expect(r.original).toBe("I has a apple.");
    expect(seen!.url).toBe("http://localhost:20128/v1/chat/completions");
    expect((seen!.init.headers as Record<string, string>).Authorization).toBe("Bearer k");
    expect(JSON.parse(seen!.init.body as string).model).toBe("m");
  });
  test("parses SSE stream bodies", async () => {
    const sse = ["I h", "ave"].map((c) => `data: ${JSON.stringify({ choices: [{ delta: { content: c } }] })}\n\n`).join("") + "data: [DONE]\n\n";
    const e = new OpenAICompatibleEngine(cfg, { fetch: fakeFetch(() => new Response(sse, { status: 200 })) });
    expect((await e.check({ text: "I h ave", dictionary: [] })).corrected).toBe("I have");
  });
  test("no auth header without key; trailing slash tolerated", async () => {
    let seen: { url: string; init: RequestInit } | undefined;
    const e = new OpenAICompatibleEngine({ ...cfg, endpoint: cfg.endpoint + "/" }, {
      fetch: fakeFetch((url, init) => ((seen = { url, init }), ok("x"))),
    });
    await e.check({ text: "x", dictionary: [] });
    expect(seen!.url).toBe("http://localhost:20128/v1/chat/completions");
    expect((seen!.init.headers as Record<string, string>).Authorization).toBeUndefined();
  });
  test("HTTP error -> EngineError http", async () => {
    const e = new OpenAICompatibleEngine(cfg, { fetch: fakeFetch(() => new Response("no", { status: 500 })) });
    await expect(e.check({ text: "a", dictionary: [] })).rejects.toMatchObject({ kind: "http" });
  });
  test("network failure -> EngineError network", async () => {
    const e = new OpenAICompatibleEngine(cfg, { fetch: (() => Promise.reject(new TypeError("fail"))) as unknown as typeof fetch });
    await expect(e.check({ text: "a", dictionary: [] })).rejects.toMatchObject({ kind: "network" });
  });
  test("timeout -> EngineError timeout", async () => {
    const slow = ((_u: string, init: RequestInit) =>
      new Promise((_res, rej) => {
        init.signal!.addEventListener("abort", () => rej(new DOMException("aborted", "AbortError")));
      })) as unknown as typeof fetch;
    const e = new OpenAICompatibleEngine({ ...cfg, timeoutMs: 30 }, { fetch: slow });
    const err = await e.check({ text: "a", dictionary: [] }).catch((x) => x);
    expect(err).toBeInstanceOf(EngineError);
    expect(err.kind).toBe("timeout");
  });
  test("empty / malformed response -> EngineError bad_response", async () => {
    const e1 = new OpenAICompatibleEngine(cfg, { fetch: fakeFetch(() => ok("   ")) });
    await expect(e1.check({ text: "a", dictionary: [] })).rejects.toMatchObject({ kind: "bad_response" });
    const e2 = new OpenAICompatibleEngine(cfg, { fetch: fakeFetch(() => new Response("{}", { status: 200 })) });
    await expect(e2.check({ text: "a", dictionary: [] })).rejects.toMatchObject({ kind: "bad_response" });
  });
  test("runaway output (>4x length) rejected", async () => {
    const e = new OpenAICompatibleEngine(cfg, { fetch: fakeFetch(() => ok("blah ".repeat(100))) });
    await expect(e.check({ text: "short text here", dictionary: [] })).rejects.toMatchObject({ kind: "bad_response" });
  });
  test("testConnection ok / fail", async () => {
    const good = new OpenAICompatibleEngine(cfg, { fetch: fakeFetch(() => ok("pong")) });
    expect((await good.testConnection()).ok).toBe(true);
    const bad = new OpenAICompatibleEngine(cfg, { fetch: fakeFetch(() => new Response("x", { status: 401 })) });
    const r = await bad.testConnection();
    expect(r.ok).toBe(false);
    expect(r.message).toContain("401");
  });
});

describe("diffWords", () => {
  test("identical -> single equal op", () => {
    expect(diffWords("a b", "a b")).toEqual([{ type: "equal", text: "a b" }]);
  });
  test("replace word", () => {
    const d = diffWords("I has a apple.", "I have an apple.");
    const del = d.filter((o) => o.type === "delete").map((o) => o.text.trim());
    const ins = d.filter((o) => o.type === "insert").map((o) => o.text.trim());
    expect(del).toEqual(["has", "a"]);
    expect(ins).toEqual(["have", "an"]);
    expect(d.map((o) => (o.type === "insert" ? "" : o.text)).join("")).toBe("I has a apple.");
    expect(d.map((o) => (o.type === "delete" ? "" : o.text)).join("")).toBe("I have an apple.");
  });
  test("preserves whitespace and newlines on reconstruct", () => {
    const a = "Hi  there,\nhow r you?";
    const b = "Hi  there,\nhow are you?";
    const d = diffWords(a, b);
    expect(d.map((o) => (o.type === "insert" ? "" : o.text)).join("")).toBe(a);
    expect(d.map((o) => (o.type === "delete" ? "" : o.text)).join("")).toBe(b);
  });
});

describe("postProcess", () => {
  test("restores dictionary casing, whole words only", () => {
    expect(postProcess("Xpiki is ok", "XPiki is ok", ["Xpiki"])).toBe("Xpiki is ok");
    expect(postProcess("x", "Xpikis", ["Xpiki"])).toBe("Xpikis");
  });
  test("straightens curly quotes unless original had them", () => {
    expect(postProcess("dont", "doesn’t", [])).toBe("doesn't");
    expect(postProcess("it’s", "it’s", [])).toBe("it’s");
  });
});

describe("parseNotes", () => {
  test("extracts bullet lines, max 6", () => {
    const raw = "<corrected>x</corrected>\n<notes>\n- has → have: chủ ngữ I\n2. a → an: trước nguyên âm\n</notes>";
    expect(parseNotes(raw)).toEqual(["has → have: chủ ngữ I", "a → an: trước nguyên âm"]);
  });
  test("missing block gives empty", () => {
    expect(parseNotes("<corrected>x</corrected>")).toEqual([]);
  });
});
