// Sets the app's version everywhere it is written: `pnpm bump 1.0.1`.
//
// The release workflow requires tauri.conf.json, apps/desktop/package.json and the app's Cargo.toml
// to agree, and CI builds with `--locked`, so Cargo.lock's entry for mayhem-desktop must follow too.
// Each file is edited in place, as text, so its formatting is kept. Nothing is committed: pushing
// the bumped version to main is what starts a release.
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const version = process.argv[2];

if (!version || !/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) {
  console.error("usage: pnpm bump <major.minor.patch>, e.g. pnpm bump 1.0.1");
  process.exit(1);
}

// Each pattern must match exactly once, and captures the text before the version.
const files = [
  ["apps/desktop/src-tauri/tauri.conf.json", /^(\s*"version":\s*")[^"]+(?=")/m],
  ["apps/desktop/package.json", /^(\s*"version":\s*")[^"]+(?=")/m],
  ["apps/desktop/src-tauri/Cargo.toml", /^(\[package\][^[]*?\nversion = ")[^"]+(?=")/],
  ["Cargo.lock", /(\nname = "mayhem-desktop"\nversion = ")[^"]+(?=")/],
];

// Read and check them all before writing any, so a failure leaves nothing half bumped.
const edits = files.map(([path, pattern]) => {
  const text = readFileSync(join(root, path), "utf8");
  const match = text.match(pattern);
  if (!match) {
    console.error(`no version found in ${path}`);
    process.exit(1);
  }
  const old = match[0].slice(match[1].length);
  return { path, old, text: text.replace(pattern, `$1${version}`) };
});

for (const { path, old, text } of edits) {
  writeFileSync(join(root, path), text);
  console.log(`${path}: ${old} -> ${version}`);
}
