// Builds the manual and the cheat sheet as PDF, in English and French, from the Markdown of
// en/ and fr/. Effect parameters come from effects.json (`--effects` refreshes it from the
// audiostrada binary), presets from the source, their French from the interface's.
//
//   node docs/manual/build.mjs [--effects] [--lang en|fr] [--html]
//
// PDFs need WeasyPrint: on the PATH, in $WEASYPRINT, or run through `uvx` when uv is installed.
import { execFileSync, spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { render, slug } from "./md.mjs";

const here = import.meta.dirname;
const repo = join(here, "../..");
const read = (p) => readFileSync(p, "utf8");
export const LANGS = ["en", "fr"];

const T = {
  en: {
    manual: "User manual",
    cheat: "Cheat sheet",
    contents: "Contents",
    chapter: "Chapter",
    parameters: "Parameters",
    presets: "Factory presets",
    parameter: "Parameter",
    range: "Range",
    default: "Default",
    choices: "Choices",
    on_off: "on / off",
    version: "Version",
    tagline: "Every tool a professional studio needs. Nothing it doesn't.",
  },
  fr: {
    manual: "Manuel utilisateur",
    cheat: "Aide-mémoire",
    contents: "Sommaire",
    chapter: "Chapitre",
    parameters: "Paramètres",
    presets: "Presets d'usine",
    parameter: "Paramètre",
    range: "Plage",
    default: "Défaut",
    choices: "Choix",
    on_off: "oui / non",
    version: "Version",
    tagline: "Tous les outils d'un studio professionnel. Rien de superflu.",
  },
};

// The interface's French, to name parameters and presets as the program does.
const french = (() => {
  const dir = join(repo, "crates/audiostrada-ui/assets/i18n/fr");
  const map = new Map();
  for (const f of readdirSync(dir)) {
    const lines = read(join(dir, f)).split("\n").filter((l) => !l.startsWith("#"));
    let entry = [];
    for (const l of [...lines, ""]) {
      if (l.trim()) entry.push(l);
      else {
        if (entry.length === 2) map.set(entry[0], entry[1]);
        entry = [];
      }
    }
  }
  return map;
})();
const tr = (lang, s) => (lang === "fr" ? (french.get(s) ?? s) : s);

const effectsFile = join(here, "effects.json");
function refreshEffects() {
  const bin = process.env.AUDIOSTRADA ?? "audiostrada";
  const input = [
    { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "manual", version: "1" } } },
    { jsonrpc: "2.0", method: "notifications/initialized" },
    { jsonrpc: "2.0", id: 2, method: "tools/call", params: { name: "list_effects", arguments: {} } },
  ].map((m) => JSON.stringify(m)).join("\n") + "\n";
  const out = execFileSync(bin, ["mcp"], { input, encoding: "utf8", stdio: ["pipe", "pipe", "ignore"] });
  const reply = out.trim().split("\n").map((l) => JSON.parse(l)).find((m) => m.id === 2);
  const effects = JSON.parse(reply.result.content[0].text);
  writeFileSync(effectsFile, JSON.stringify(effects, null, 1) + "\n");
  console.log(`effects.json: ${effects.length} effects`);
}

const effects = () => JSON.parse(read(effectsFile));

// Factory preset names of each effect, from presets.rs.
const presets = (() => {
  const src = read(join(repo, "crates/audiostrada-ui/src/presets.rs"));
  const out = {};
  const blocks = src.split(/\n        "([a-z_]+)" => vec!\[/);
  for (let i = 1; i < blocks.length; i += 2) {
    out[blocks[i]] = [...blocks[i + 1].matchAll(/tr\("([^"]+)"\)/g)].map((m) => m[1]);
  }
  return out;
})();

const number = (v) => (Number.isInteger(v) ? String(v) : String(Math.round(v * 100) / 100));

function effectBlock(lang, id) {
  const t = T[lang];
  const e = effects().find((x) => x.id === id);
  if (!e) throw new Error(`no effect ${id} in effects.json`);
  const rows = e.params
    .filter((p) => !/^p\d_/.test(p.id))
    // Bands, steps and oscillators alike show once, as "n".
    .filter((p) => !/\b(Band|Step|Osc|LFO|Modulation|Macro) ([2-9]|\d\d)\b/.test(p.name))
    .map((p) => {
      const unit = p.unit ? ` ${p.unit}` : "";
      let range, def;
      if (p.choices) {
        range = p.choices.map((c) => tr(lang, c)).join(", ");
        def = tr(lang, p.choices[Math.round(p.default)] ?? "");
      } else if (p.min === 0 && p.max === 1 && !p.unit) {
        range = t.on_off;
        def = p.default >= 0.5 ? t.on_off.split(" / ")[0] : t.on_off.split(" / ")[1];
      } else {
        range = `${number(p.min)} – ${number(p.max)}${unit}`;
        def = `${number(p.default)}${unit}`;
      }
      const name = tr(lang, p.name).replace(/\b(Band|Bande|Step|Pas|Osc|LFO|Modulation|Macro) 1\b/i, "$1 n");
      return `<tr><td>${name}</td><td>${range}</td><td>${def}</td></tr>`;
    })
    .join("");
  let html = `<div class="params"><h4>${t.parameters}</h4><table class="ref"><thead><tr><th>${t.parameter}</th><th>${t.range}</th><th>${t.default}</th></tr></thead><tbody>${rows}</tbody></table></div>`;
  const list = presets[id];
  if (list?.length) {
    const groups = new Map();
    for (const name of list.map((n) => tr(lang, n))) {
      const [cat, rest] = name.includes(": ") ? name.split(/\s*:\s+/, 2) : ["", name];
      if (!groups.has(cat)) groups.set(cat, []);
      groups.get(cat).push(rest);
    }
    html += `<div class="presets"><h4>${t.presets}</h4>${[...groups]
      .map(([cat, names]) => `<p>${cat ? `<strong>${cat}</strong> ` : ""}${names.join(" · ")}</p>`)
      .join("")}</div>`;
  }
  return html;
}

export function chapters(lang, { image } = {}) {
  const dir = join(here, lang);
  return readdirSync(dir)
    .filter((f) => /^\d\d-.*\.md$/.test(f))
    .sort()
    .map((file) => {
      const text = read(join(dir, file));
      const { html, headings } = render(text, {
        image,
        prefix: file.replace(/^\d\d-|\.md$/g, ""),
        directive: (name, arg) => (name === "effect" ? effectBlock(lang, arg) : ""),
      });
      const title = headings.find((h) => h.level === 1)?.text ?? file;
      return { file, slug: file.replace(/^\d\d-|\.md$/g, ""), title, html, headings };
    });
}

export function cheatsheet(lang, opts = {}) {
  return render(read(join(here, lang, "cheatsheet.md")), opts);
}

// The site's waves, drawn once: a few seeded lines quiet at the edges, the middle one amber.
function waves(width, height, { lines = 11, center = 0.5, spread = 0.035, seed = 7 } = {}) {
  let s = seed;
  const rand = () => ((s = (s * 16807) % 2147483647) / 2147483647);
  const paths = [];
  for (let i = 0; i < lines; i++) {
    const lead = i === Math.floor(lines / 2);
    const base = center + (i - (lines - 1) / 2) * spread;
    const amp = 0.06 + rand() * 0.09;
    const freq = [1.4 + rand() * 1.6, 3 + rand() * 3, 7 + rand() * 6];
    const phase = rand() * Math.PI * 2;
    const color = lead ? "#ffb547" : i % 3 === 0 ? "#8cc4ff" : "#5aa2ff";
    const alpha = lead ? 0.85 : 0.18 + rand() * 0.3;
    let d = "";
    for (let x = 0; x <= width; x += 4) {
      const u = x / width;
      const env = Math.sin(Math.PI * u) ** 1.5 * (0.6 + 0.8 * Math.exp(-(((u - 0.62) / 0.28) ** 2)));
      let y = 0;
      for (let k = 0; k < 3; k++) y += Math.sin(u * freq[k] * Math.PI * 2 + phase * (k + 1)) / (k + 1.4);
      d += `${x ? "L" : "M"}${x.toFixed(1)} ${(height * (base + amp * env * y)).toFixed(1)}`;
    }
    // A glow without filters: wide faint strokes under the line.
    for (const [w, a] of [[9, 0.05], [4, 0.12], [lead ? 2.2 : 1.2, 1]]) {
      paths.push(`<path d="${d}" fill="none" stroke="${color}" stroke-opacity="${(alpha * a).toFixed(3)}" stroke-width="${w}" stroke-linecap="round"/>`);
    }
  }
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${width} ${height}" preserveAspectRatio="none">${paths.join("")}</svg>`;
}

const version = () => read(join(repo, "Cargo.toml")).match(/^version = "([^"]+)"/m)?.[1] ?? "";
const fileUrl = (p) => pathToFileURL(p).href;
const logo = () => read(join(here, "assets/logo.svg"));
const date = (lang) =>
  new Date().toLocaleDateString(lang === "fr" ? "fr-FR" : "en-GB", { year: "numeric", month: "long" });

function fonts() {
  const f = (p) => fileUrl(join(repo, p));
  return `<style>
@font-face { font-family: "Archivo"; src: url("${f("docs/manual/assets/fonts/archivo-latin-wdth-normal.woff2")}"); font-weight: 100 900; font-stretch: 62% 125%; }
@font-face { font-family: "Archivo"; src: url("${f("docs/manual/assets/fonts/archivo-latin-ext-wdth-normal.woff2")}"); font-weight: 100 900; font-stretch: 62% 125%; unicode-range: U+0100-02BA, U+1E00-1E9F; }
@font-face { font-family: "Inter"; src: url("${f("crates/audiostrada-ui/assets/fonts/Inter-Regular.ttf")}"); font-weight: 400; }
@font-face { font-family: "Inter"; src: url("${f("crates/audiostrada-ui/assets/fonts/Inter-SemiBold.ttf")}"); font-weight: 600 700; }
</style>`;
}

function manualHtml(lang) {
  const t = T[lang];
  const image = (src) => fileUrl(join(here, src));
  const list = chapters(lang, { image });
  const toc = list
    .map((c, n) => {
      const h1 = c.headings.find((h) => h.level === 1);
      const subs = c.headings
        .filter((h) => h.level === 2)
        .map((h) => `<li class="sub"><a href="#${h.id}">${h.text.replace(/\*|`/g, "")}</a></li>`)
        .join("");
      return `<li class="chap"><a href="#${h1.id}"><span class="num">${n + 1}</span>${h1.text}</a></li>${subs}`;
    })
    .join("");
  const body = list
    .map((c, n) => {
      const opener = `<div class="opener"><div class="num">${t.chapter} ${n + 1}</div>${waves(600, 60, { lines: 7, spread: 0.06, seed: 11 + n })}</div>`;
      return `<section class="chapter" data-title="${c.title}">${opener}${c.html}</section>`;
    })
    .join("\n");
  const cover = `<section class="cover">
<div class="waves">${waves(1200, 700, { seed: 3 })}</div>
<div class="brand">${logo()}<span>AudioStrada</span></div>
<div class="title"><h1 class="cover-title">${t.manual}</h1><p>${t.tagline}</p></div>
<div class="meta">${t.version} ${version()} · ${date(lang)} · audiostrada.com</div>
</section>`;
  return `<!doctype html><html lang="${lang}"><head><meta charset="utf-8"><title>AudioStrada – ${t.manual}</title>
${fonts()}<link rel="stylesheet" href="${fileUrl(join(here, "style/manual.css"))}"></head><body>
${cover}
<nav class="toc"><h2 class="toc-title">${t.contents}</h2><ol>${toc}</ol></nav>
${body}
</body></html>`;
}

function cheatHtml(lang) {
  const t = T[lang];
  const { html } = cheatsheet(lang);
  return `<!doctype html><html lang="${lang}"><head><meta charset="utf-8"><title>AudioStrada – ${t.cheat}</title>
${fonts()}<link rel="stylesheet" href="${fileUrl(join(here, "style/cheatsheet.css"))}"></head><body>
<header><div class="waves">${waves(1400, 120, { lines: 9, spread: 0.07, seed: 5 })}</div>
<div class="brand">${logo()}<span>AudioStrada</span></div><h1>${t.cheat}</h1><div class="meta">${t.version} ${version()} · audiostrada.com</div></header>
<main>${html}</main></body></html>`;
}

function weasyprint(html, pdf) {
  const tries = process.env.WEASYPRINT
    ? [[process.env.WEASYPRINT, []]]
    : [["weasyprint", []], ["uvx", ["weasyprint"]]];
  for (const [cmd, pre] of tries) {
    const r = spawnSync(cmd, [...pre, "-q", html, pdf], { stdio: ["ignore", "inherit", "inherit"] });
    if (!r.error) {
      if (r.status !== 0) throw new Error(`weasyprint failed on ${html}`);
      return;
    }
  }
  throw new Error("WeasyPrint not found: install it (pipx install weasyprint) or set $WEASYPRINT");
}

if (process.argv[1] === import.meta.filename) {
  const args = process.argv.slice(2);
  if (args.includes("--effects")) refreshEffects();
  const langs = args.includes("--lang") ? [args[args.indexOf("--lang") + 1]] : LANGS;
  const dist = join(here, "dist");
  mkdirSync(dist, { recursive: true });
  for (const lang of langs) {
    for (const [kind, html, name] of [
      ["manual", manualHtml(lang), lang === "fr" ? "AudioStrada-Manuel" : "AudioStrada-Manual"],
      ["cheatsheet", cheatHtml(lang), lang === "fr" ? "AudioStrada-Aide-memoire" : "AudioStrada-Cheat-Sheet"],
    ]) {
      const file = join(dist, `${kind}-${lang}.html`);
      writeFileSync(file, html);
      if (args.includes("--html")) continue;
      const pdf = join(dist, `${name}-${lang}.pdf`);
      weasyprint(file, pdf);
      console.log(pdf);
    }
  }
}

export { slug };
