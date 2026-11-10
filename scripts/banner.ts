// Renders the README banners. Needs a Chromium-based browser, and Microsoft Edge is used here.
//
//     bun scripts/banner.ts

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { chromium } from "playwright-core";
import sharp from "sharp";

const root = join(import.meta.dirname, "..");
const font = (path: string) =>
  readFileSync(join(root, "node_modules/@fontsource-variable", path)).toString("base64");
const logo = readFileSync(join(root, "packaging/disweep.svg"), "utf8").replace(
  "<title>Disweep</title>",
  "",
);

const palettes = {
  dark: {
    bg: "#05070c",
    glow: "#0f2a55",
    dots: "rgba(255,255,255,.05)",
    ink: "#f4f6fb",
    soft: "#7c8598",
    panel: "rgba(255,255,255,.035)",
    line: "rgba(255,255,255,.1)",
    number: "#57b0ff",
    good: "#3dd68c",
    track: "rgba(255,255,255,.08)",
  },
  light: {
    bg: "#f7f8fc",
    glow: "#d8e8ff",
    dots: "rgba(18,21,29,.07)",
    ink: "#12151d",
    soft: "#5a6376",
    panel: "rgba(255,255,255,.85)",
    line: "rgba(18,21,29,.12)",
    number: "#2479e0",
    good: "#0f8354",
    track: "rgba(18,21,29,.08)",
  },
};

function page(palette: (typeof palettes)["dark"]): string {
  const tile = (value: string, label: string) =>
    `<div class="tile"><b>${value}</b><span>${label}</span></div>`;
  return `<!doctype html><html><head><style>
@font-face{font-family:G;src:url(data:font/woff2;base64,${font("geist/files/geist-latin-wght-normal.woff2")});font-weight:100 900}
@font-face{font-family:M;src:url(data:font/woff2;base64,${font("geist-mono/files/geist-mono-latin-wght-normal.woff2")});font-weight:100 900}
*{box-sizing:border-box}
body{margin:0;width:1280px;height:400px;overflow:hidden;background:${palette.bg};color:${palette.ink};font-family:G;display:flex;align-items:center;justify-content:space-between;padding:0 72px 0 88px;position:relative}
body::before{content:"";position:absolute;inset:0;background:radial-gradient(760px 420px at 0% 0%,${palette.glow},transparent 70%),radial-gradient(circle,${palette.dots} 1px,transparent 1.2px) 0 0/22px 22px}
.left,.right{position:relative}
.brand{display:flex;align-items:center;gap:20px}
.brand svg{width:84px;height:84px}
h1{margin:0;font-size:92px;font-weight:700;letter-spacing:-.045em;line-height:1}
.grad{background:linear-gradient(100deg,#57b0ff,#8b7cf6);-webkit-background-clip:text;color:transparent}
h2{margin:30px 0 0;font-size:34px;font-weight:650;letter-spacing:-.02em}
h2 span{color:${palette.soft}}
.chips{display:flex;gap:10px;margin-top:30px}
.chip{font:400 16px M;padding:9px 15px;border:1px solid ${palette.line};border-radius:999px;background:${palette.panel}}
.chip em{font-style:normal;color:${palette.number}}
.right{width:440px}
.card{border:1px solid ${palette.line};border-radius:20px;background:${palette.panel};padding:22px}
.eyebrow{font:400 14px M;letter-spacing:.14em;color:${palette.soft}}
.row{display:flex;justify-content:space-between;align-items:center;font:400 16px M;border:1px solid ${palette.line};border-radius:12px;padding:12px 15px;margin-top:14px}
.row i{font-style:normal;color:${palette.soft};font-size:14px}
.tiles{display:grid;grid-template-columns:repeat(4,1fr);margin-top:18px;text-align:center}
.tile b{display:block;font-size:26px;font-weight:650;color:${palette.number};letter-spacing:-.02em}
.tile span{font:400 11.5px M;letter-spacing:.12em;color:${palette.soft}}
.track{height:7px;border-radius:9px;background:${palette.track};margin-top:18px;overflow:hidden}
.fill{height:100%;width:82%;border-radius:9px;background:linear-gradient(90deg,#4fa8ff,#8b7cf6)}
.foot{display:flex;justify-content:space-between;font:400 14.5px M;color:${palette.soft};margin-top:10px}
.foot b{font-weight:500;color:${palette.good}}
.confirm{margin-top:14px;display:flex;justify-content:space-between;align-items:center;padding:13px 18px;font:400 15px M;color:${palette.soft}}
.confirm b{font-weight:500;color:${palette.number};border:1px solid ${palette.number};border-radius:9px;padding:5px 10px;font-size:13.5px;letter-spacing:.12em}
</style></head><body>
<div class="left">
  <div class="brand">${logo}<h1>Di<span class="grad">sweep</span></h1></div>
  <h2>Sweep your messages. <span>Review it first.</span></h2>
  <div class="chips"><div class="chip"><em>preview</em> by default</div><div class="chip">/sweep in any DM</div><div class="chip">Windows · macOS</div></div>
</div>
<div class="right">
  <div class="card">
    <div class="eyebrow">03 / SWEEPING</div>
    <div class="row"><span>Weekend plans</span><i>only yours</i></div>
    <div class="tiles">${tile("1,052", "DELETED")}${tile("7", "KEPT")}${tile("58", "PER MIN")}${tile("4m", "LEFT")}</div>
    <div class="track"><div class="fill"></div></div>
    <div class="foot"><span>Sweeping at Discord's pace</span><b>82%</b></div>
  </div>
  <div class="card confirm"><span>type SWEEP to confirm</span><b>NO SURPRISES</b></div>
</div>
</body></html>`;
}

const browser = await chromium.launch({ channel: "msedge", headless: true });
try {
  for (const [name, palette] of Object.entries(palettes)) {
    const context = await browser.newContext({
      viewport: { width: 1280, height: 400 },
      deviceScaleFactor: 2,
    });
    const tab = await context.newPage();
    await tab.setContent(page(palette));
    await tab.evaluate(() => document.fonts.ready.then(() => undefined));
    const png = await tab.screenshot({ type: "png" });
    await sharp(png)
      .webp({ quality: 90 })
      .toFile(join(root, `docs/assets/banner-${name}.webp`));
    await context.close();
  }
} finally {
  await browser.close();
}
