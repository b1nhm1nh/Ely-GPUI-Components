// Usage: npm i playwright-core, then node scripts/webbench.mjs <scripts/web.sh out> <csv> [chrome|msedge] [page]; build with ELY_GALLERY_ASSETS=http://localhost:8080/assets/.
// Serves the web gallery and times every page in Chrome, headed so the GPU and vsync are real.
import { createServer } from "node:http";
import { readFile, writeFile } from "node:fs/promises";
import { extname, join } from "node:path";
import { chromium } from "playwright-core";

const [root, out, browser = "chrome", page = ""] = process.argv.slice(2);
const types = { ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm", ".png": "image/png", ".jpg": "image/jpeg", ".svg": "image/svg+xml", ".json": "application/json" };
const server = createServer(async (req, res) => {
  let path = decodeURIComponent(new URL(req.url, "http://x").pathname); if (path === "/") path = "/index.html";
  try {
    const body = await readFile(join(root, path));
    res.writeHead(200, { "content-type": types[extname(path)] ?? "application/octet-stream" });
    res.end(body);
  } catch { res.writeHead(404); res.end(); }
}).listen(8080);

const b = await chromium.launch({ channel: browser, headless: false, args: ["--window-size=1300,900"] });
const ctx = await b.newContext({ viewport: { width: 1280, height: 820 }, deviceScaleFactor: 1 });
const tab = await ctx.newPage();
tab.on("console", (m) => { const t = m.text(); if (/bench|ERROR|panicked/i.test(t)) console.log(t.replace(/\s+$/, "")); });
tab.on("pageerror", (e) => console.log("pageerror", e.message));
const started = Date.now();
await tab.goto(`http://localhost:8080/?bench=1${page ? `&page=${page}` : ""}`);
await tab.waitForFunction(() => "elyBench" in window || "elyBenchError" in window, null, { timeout: 30 * 60_000, polling: 1000 });
const { csv, error, gpu } = await tab.evaluate(() => {
  const gl = document.createElement("canvas").getContext("webgl2");
  const ext = gl?.getExtension("WEBGL_debug_renderer_info");
  return { csv: window.elyBench, error: window.elyBenchError, gpu: ext ? gl.getParameter(ext.UNMASKED_RENDERER_WEBGL) : "?", };
});
console.log(`gpu: ${gpu}; webgpu: ${await tab.evaluate(() => !!navigator.gpu)}; took ${((Date.now() - started) / 1000).toFixed(0)} s`);
if (error) { console.log("bench failed:", error); process.exitCode = 1; } else await writeFile(out, csv);
await b.close(); server.close();
