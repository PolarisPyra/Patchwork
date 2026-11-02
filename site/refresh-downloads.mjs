#!/usr/bin/env node
// Points the page at the newest release on the update server.
//
// The download server publishes `latest.json` for the updater, and per-tag folders
// (`/v26.11.0/Patchwork_amd64.AppImage`, ...) that are never overwritten. The page links into
// those folders, so it has to be told which tag is current: run this after a release.
//
//   node site/refresh-downloads.mjs           write the new tag into index.html
//   node site/refresh-downloads.mjs --check   exit 1 if index.html is out of date
//
// The update server sends no CORS headers, so a browser cannot read latest.json itself; this runs
// on the command line (or in a workflow) instead.

import { readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const LATEST = "https://updates.patchworkeditor.com/latest.json";
const page = fileURLToPath(new URL("./index.html", import.meta.url));
const checkOnly = process.argv.includes("--check");

const assets = {
  linux: "Patchwork_amd64.AppImage",
  windows: "Patchwork_x64-setup.exe",
  macos: "Patchwork_aarch64.dmg",
};

const response = await fetch(LATEST, { headers: { accept: "application/json" } });
if (!response.ok) {
  console.error(`latest.json answered ${response.status}`);
  process.exit(1);
}

const release = await response.json();
const tag = release.tag_name;
if (typeof tag !== "string" || !/^v\d+\.\d+\.\d+$/.test(tag)) {
  console.error(`latest.json has an unexpected tag_name: ${JSON.stringify(tag)}`);
  process.exit(1);
}

const published = new Set(release.assets?.map((asset) => asset.name) ?? []);
const missing = Object.values(assets).filter((name) => !published.has(name));
if (missing.length > 0) {
  console.error(`The release is missing ${missing.join(", ")}`);
  process.exit(1);
}

const before = await readFile(page, "utf8");
const after = before.replace(
  /updates\.patchworkeditor\.com\/v\d+\.\d+\.\d+\//g,
  `updates.patchworkeditor.com/${tag}/`,
);

if (before === after) {
  console.log(`index.html already points at ${tag}`);
  process.exit(0);
}

if (checkOnly) {
  console.error(`index.html is out of date: run node site/refresh-downloads.mjs`);
  process.exit(1);
}

await writeFile(page, after);
console.log(`index.html now points at ${tag}`);
