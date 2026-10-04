// Sets the app's version everywhere it is written: `pnpm bump 1.0.1`, or `pnpm bump major`,
// `pnpm bump minor`, `pnpm bump patch` (or `fix`) to step the current version.
//
// The release workflow requires tauri.conf.json, apps/desktop/package.json and the app's Cargo.toml
// to agree, and CI builds with `--locked`, so Cargo.lock's entry for mayhem-desktop must follow too.
// Each file is edited in place, as text, so its formatting is kept. Nothing is committed: pushing
// the bumped version to main is what starts a release.
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const arg = process.argv[2];
const steps = { major: 0, minor: 1, patch: 2, fix: 2 };
const exact = /^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/;

if (!arg || !(arg in steps || exact.test(arg))) {
  console.error("usage: pnpm bump <major|minor|patch|fix|x.y.z>, e.g. pnpm bump minor or pnpm bump 1.0.1");
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
const found = files.map(([path, pattern]) => {
  const text = readFileSync(join(root, path), "utf8");
  const match = text.match(pattern);
  if (!match) {
    console.error(`no version found in ${path}`);
    process.exit(1);
  }
  return { path, pattern, text, old: match[0].slice(match[1].length) };
});

let version = arg;
if (arg in steps) {
  // A step needs one current version to start from, so the files must already agree.
  const current = found[0].old;
  const disagree = found.filter((f) => f.old !== current);
  if (disagree.length) {
    for (const f of found) console.error(`${f.path}: ${f.old}`);
    console.error("the versions disagree; set one explicitly with pnpm bump <x.y.z>");
    process.exit(1);
  }
  const parts = current.match(/^(\d+)\.(\d+)\.(\d+)/);
  if (!parts) {
    console.error(`cannot step ${current}; set one explicitly with pnpm bump <x.y.z>`);
    process.exit(1);
  }
  // Any pre-release suffix is dropped, and the parts after the stepped one reset to zero.
  const numbers = parts.slice(1).map(Number);
  const at = steps[arg];
  numbers[at] += 1;
  numbers.fill(0, at + 1);
  version = numbers.join(".");
}

for (const { path, pattern, text, old } of found) {
  writeFileSync(join(root, path), text.replace(pattern, `$1${version}`));
  console.log(`${path}: ${old} -> ${version}`);
}
