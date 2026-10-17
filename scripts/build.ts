import { cpSync, mkdirSync, rmSync } from "node:fs";

rmSync("dist", { recursive: true, force: true });
mkdirSync("dist/renderer", { recursive: true });

async function bundle(opts: Parameters<typeof Bun.build>[0]) {
  const r = await Bun.build(opts);
  if (!r.success) {
    for (const l of r.logs) console.error(l);
    process.exit(1);
  }
}

await bundle({ entrypoints: ["src/main/main.ts"], outdir: "dist", target: "node", format: "esm", external: ["electron"] });
await bundle({ entrypoints: ["src/main/shots.ts"], outdir: "dist", target: "node", format: "esm", external: ["electron"] });
await bundle({ entrypoints: ["src/main/preload.ts"], outdir: "dist", target: "node", format: "cjs", external: ["electron"], naming: "preload.cjs" });
await bundle({ entrypoints: ["src/renderer/popover.ts", "src/renderer/settings.ts"], outdir: "dist/renderer", target: "browser", format: "iife" });
for (const f of ["popover.html", "popover.css", "settings.html", "settings.css", "theme.css"]) cpSync(`src/renderer/${f}`, `dist/renderer/${f}`);
console.log("built");
