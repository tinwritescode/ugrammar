// Builds a release the updater can install: the .app signed with the stable self-signed cert,
// its updater archive and signature, latest.json, and the zips the landing page links to.
import { $ } from "bun";
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const REPO = "tinwritescode/ugrammar";
const keys = process.env.UGRAMMAR_SIGNING_DIR ?? join(homedir(), ".ugrammar-signing");
const rcodesign = Bun.which("rcodesign") ?? join(homedir(), ".cargo/bin/rcodesign");
for (const f of ["key.pem", "cert.pem", "updater.key"]) {
  if (!existsSync(join(keys, f))) throw new Error(`missing ${join(keys, f)}`);
}
if (!existsSync(rcodesign)) throw new Error("rcodesign not found: cargo install apple-codesign");

const { version } = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));
const bundleDir = "target/release/bundle/macos";
const app = join(bundleDir, "ugrammar.app");

await $`bun run helper`;
await $`bunx tauri build --bundles app`;
// Accessibility grants follow the designated requirement; a stable cert keeps it the same across updates.
await $`${rcodesign} sign --pem-source ${join(keys, "key.pem")} --pem-source ${join(keys, "cert.pem")} ${app}`;
await $`codesign --verify --deep --strict ${app}`;
console.log((await $`codesign -d -r- ${app} 2>&1`.text()).trim().split("\n").at(-1));

rmSync("release", { recursive: true, force: true });
mkdirSync("release");
const archive = "release/ugrammar.app.tar.gz";
await $`tar -czf ${archive} -C ${bundleDir} ugrammar.app`.env({ ...process.env, COPYFILE_DISABLE: "1" });
await $`bunx tauri signer sign -f ${join(keys, "updater.key")} -p "" ${archive}`;

const latest = {
  version,
  pub_date: new Date().toISOString(),
  platforms: {
    "darwin-aarch64": {
      signature: readFileSync(`${archive}.sig`, "utf8").trim(),
      url: `https://github.com/${REPO}/releases/download/v${version}/ugrammar.app.tar.gz`,
    },
  },
};
writeFileSync("release/latest.json", JSON.stringify(latest, null, 2) + "\n");

await $`ditto -c -k --keepParent ${app} release/ugrammar-macos-arm64.zip`;
copyFileSync("release/ugrammar-macos-arm64.zip", `release/ugrammar-${version}-arm64.zip`);
console.log(`release/ ready for v${version}`);
