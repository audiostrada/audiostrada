// The Markdown the manual uses, to HTML: headings with anchors, paragraphs, nested lists,
// tables, quotes (> **Tip** becomes a note), code, images as figures, [[Ctrl+S]] as keys.

export const slug = (text) =>
  text
    .toLowerCase()
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .replace(/<[^>]+>/g, "")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");

const escape = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

export function inline(text, { image } = {}) {
  const codes = [];
  let s = text.replace(/`([^`]+)`/g, (_, c) => `\u0000${codes.push(`<code>${escape(c)}</code>`) - 1}\u0000`);
  s = escape(s)
    .replace(/\[\[([^\]]+)\]\]/g, (_, k) =>
      k.split(/\s*\+\s*(?=\S)/).map((p) => `<kbd>${p}</kbd>`).join("<span class=plus>+</span>"),
    )
    .replace(/!\[([^\]]*)\]\(([^)]+)\)/g, (_, alt, src) => `<img src="${image ? image(src) : src}" alt="${alt}">`)
    .replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<a href="$2">$1</a>')
    .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
    .replace(/(^|[^*\w])\*([^*\s][^*]*)\*/g, "$1<em>$2</em>")
    .replace(/ -- /g, " – ");
  return s.replace(/\u0000(\d+)\u0000/g, (_, i) => codes[i]);
}

/// Blocks of `text`, with `directive(name, arg)` for <!-- name:arg --> lines.
export function render(text, opts = {}) {
  const lines = text.replace(/\r/g, "").split("\n");
  const out = [];
  const headings = [];
  let i = 0;
  const para = [];
  const flush = () => {
    if (para.length) out.push(`<p>${inline(para.join(" "), opts)}</p>`);
    para.length = 0;
  };
  const listAt = (start) => {
    // Items with their indentation; deeper ones nest in the item before them.
    const items = [];
    let j = start;
    while (j < lines.length) {
      const m = lines[j].match(/^(\s*)([-*]|\d+\.)\s+(.*)$/);
      if (m) {
        items.push({ depth: m[1].length, ordered: /\d/.test(m[2]), text: [m[3]] });
      } else if (/^\s+\S/.test(lines[j]) && items.length) {
        items[items.length - 1].text.push(lines[j].trim());
      } else break;
      j++;
    }
    const build = (from, depth) => {
      const tag = items[from].ordered ? "ol" : "ul";
      let html = `<${tag}>`;
      let k = from;
      while (k < items.length && items[k].depth >= depth) {
        if (items[k].depth > depth) {
          const [inner, next] = build(k, items[k].depth);
          html = html.replace(/<\/li>$/, `${inner}</li>`);
          k = next;
          continue;
        }
        html += `<li>${inline(items[k].text.join(" "), opts)}</li>`;
        k++;
      }
      return [`${html}</${tag}>`, k];
    };
    return [build(0, items[0].depth)[0], j];
  };
  while (i < lines.length) {
    const line = lines[i];
    let m;
    if (!line.trim()) {
      flush();
      i++;
    } else if ((m = line.match(/^<!--\s*([\w-]+):?\s*([\w-]*)\s*-->$/))) {
      flush();
      if (opts.directive) out.push(opts.directive(m[1], m[2]) ?? "");
      i++;
    } else if ((m = line.match(/^(#{1,4})\s+(.*?)\s*(?:\{#([\w-]+)\})?$/))) {
      flush();
      const level = m[1].length;
      const own = slug(m[2]);
      const id = m[3] ?? (opts.prefix ? (level === 1 ? opts.prefix : `${opts.prefix}-${own}`) : own);
      headings.push({ level, text: m[2], id });
      out.push(`<h${level} id="${id}">${inline(m[2], opts)}</h${level}>`);
      i++;
    } else if (line.startsWith("```")) {
      flush();
      const body = [];
      for (i++; i < lines.length && !lines[i].startsWith("```"); i++) body.push(lines[i]);
      out.push(`<pre><code>${escape(body.join("\n"))}</code></pre>`);
      i++;
    } else if (line.startsWith("|")) {
      flush();
      const rows = [];
      for (; i < lines.length && lines[i].startsWith("|"); i++) rows.push(lines[i]);
      const cells = (r) => r.replace(/^\||\|$/g, "").split(/(?<!\\)\|/).map((c) => inline(c.trim().replace(/\\\|/g, "|"), opts));
      const head = cells(rows[0]);
      const body = rows.slice(2).map(cells);
      out.push(
        `<table><thead><tr>${head.map((c) => `<th>${c}</th>`).join("")}</tr></thead><tbody>${body
          .map((r) => `<tr>${r.map((c) => `<td>${c}</td>`).join("")}</tr>`)
          .join("")}</tbody></table>`,
      );
    } else if (line.startsWith(">")) {
      flush();
      const body = [];
      for (; i < lines.length && lines[i].startsWith(">"); i++) body.push(lines[i].replace(/^>\s?/, ""));
      const kind = body[0].match(/^\*\*(Tip|Note|Warning|Astuce|Remarque|Attention)\*\*/);
      const cls = kind ? { Tip: "tip", Astuce: "tip", Note: "note", Remarque: "note" }[kind[1]] ?? "warning" : "note";
      out.push(`<aside class="${cls}">${render(body.join("\n"), opts).html}</aside>`);
    } else if ((m = line.match(/^!\[([^\]]*)\]\(([^)]+)\)$/))) {
      flush();
      const src = opts.image ? opts.image(m[2]) : m[2];
      out.push(`<figure><img src="${src}" alt="${escape(m[1])}">${m[1] ? `<figcaption>${inline(m[1], opts)}</figcaption>` : ""}</figure>`);
      i++;
    } else if (/^\s*([-*]|\d+\.)\s+/.test(line) && !para.length) {
      const [html, next] = listAt(i);
      out.push(html);
      i = next;
    } else {
      para.push(line.trim());
      i++;
    }
  }
  flush();
  return { html: out.join("\n"), headings };
}
