import { cpSync, mkdirSync, rmSync } from "node:fs";

rmSync("dist", { recursive: true, force: true });
mkdirSync("dist", { recursive: true });

const pages = ["popover", "tray", "badge", "underline"];
const r = await Bun.build({ entrypoints: pages.map((p) => `src/renderer/${p}.ts`), outdir: "dist", target: "browser", format: "iife" });
if (!r.success) {
  for (const l of r.logs) console.error(l);
  process.exit(1);
}
for (const f of [...pages.flatMap((p) => [`${p}.html`, `${p}.css`]), "theme.css"]) cpSync(`src/renderer/${f}`, `dist/${f}`);
console.log("built");
