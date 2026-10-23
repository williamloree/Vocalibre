// Fails unless the tag (vX.Y.Z) matches the version in Cargo.toml,
// tauri.conf.json and package.json. Usage: node scripts/check-version.mjs v1.2.3
import { readFileSync } from "node:fs";

const tag = process.argv[2] ?? "";
const read = (path) => readFileSync(new URL(`../${path}`, import.meta.url), "utf8");

const cargo = read("Cargo.toml").match(/\[workspace\.package\][^[]*?\nversion = "([^"]+)"/)?.[1];
const versions = {
  tag: tag.replace(/^v/, ""),
  "Cargo.toml": cargo,
  "src-tauri/tauri.conf.json": JSON.parse(read("src-tauri/tauri.conf.json")).version,
  "package.json": JSON.parse(read("package.json")).version,
};

const expected = versions.tag;
const wrong = Object.entries(versions).filter(([, v]) => v !== expected);
if (!/^v\d+\.\d+\.\d+$/.test(tag) || wrong.length > 0) {
  console.error(`Version mismatch (tag ${tag || "missing"}):`);
  for (const [file, v] of Object.entries(versions)) console.error(`  ${file}: ${v}`);
  process.exit(1);
}
console.log(`Version ${expected} OK`);
