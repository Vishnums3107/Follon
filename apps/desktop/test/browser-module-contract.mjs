import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const testDirectory = dirname(fileURLToPath(import.meta.url));
const appDirectory = resolve(testDirectory, "..");
const index = await readFile(resolve(appDirectory, "index.html"), "utf8");
const styles = await readFile(resolve(appDirectory, "styles.css"), "utf8");
const favicon = await readFile(resolve(appDirectory, "favicon.svg"), "utf8");

assert.match(index, /<script type="module" src="\/dist\/main\.js"><\/script>/u);
for (const id of ["workspace-detail", "workspace-summary", "workspace-canvas", "refresh-workspace"]) {
  assert.ok(index.includes(`id="${id}"`), `missing workspace control ${id}`);
}

for (const font of ["SF Pro Display", "SF Pro Text", "SF Compact", "SF Mono"]) {
  assert.ok(styles.includes(font), `missing requested system font mapping: ${font}`);
}
const colorTokens = [...`${styles}\n${favicon}`.matchAll(/#[0-9a-fA-F]{3,8}\b/gu)].map((match) => match[0].toLowerCase());
assert.ok(colorTokens.every((color) => color === "#000" || color === "#fff"), `non-monochrome color found: ${colorTokens.join(", ")}`);
assert.match(styles, /body\s*\{[^}]*background:\s*var\(--white\)/su);
assert.match(styles, /\.workspace-detail\s*\{[^}]*background:\s*var\(--black\)/su);

const visited = new Set();
async function verifyModule(relativePath) {
  if (visited.has(relativePath)) return;
  visited.add(relativePath);
  const source = await readFile(resolve(appDirectory, "dist", relativePath), "utf8");
  for (const match of source.matchAll(/from\s+["'](\.\/.+?)["']/gu)) {
    const specifier = match[1];
    assert.ok(specifier.endsWith(".js"), `${relativePath} has a browser-incompatible import: ${specifier}`);
    await verifyModule(specifier.slice(2));
  }
}

await verifyModule("main.js");
assert.deepEqual([...visited].sort(), ["catalog.js", "evidence.js", "main.js", "workspaces.js"]);
console.log("Browser module graph / workspace shell contract passed");
