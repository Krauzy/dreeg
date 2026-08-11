import { readFileSync } from "node:fs";

const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");
const tauri = JSON.parse(readFileSync(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"));
const htmlPolicy = html.match(/http-equiv="Content-Security-Policy"[\s\S]*?content="([^"]+)"/)?.[1];
const tauriPolicy = tauri.app?.security?.csp;

for (const [source, policy] of [["index.html", htmlPolicy], ["tauri.conf.json", tauriPolicy]]) {
  if (!policy) throw new Error(`${source} does not define a Content Security Policy.`);
  const imageDirective = policy.split(";").find((directive) => directive.trim().startsWith("img-src"));
  if (!imageDirective?.split(/\s+/).includes("data:")) {
    throw new Error(`${source} must allow data: in img-src for locally generated item artwork.`);
  }
}

console.log("CSP check passed: HTML and Tauri both allow data: item artwork.");
