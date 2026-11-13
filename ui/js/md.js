// Markdown ringan dan aman (input di-escape dulu): heading, list, tabel, kode, inline.
const escH = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
const inline = (t) =>
  t
    .replace(/`([^`\n]+)`/g, "<code>$1</code>")
    .replace(/\*\*([^*\n]+)\*\*/g, "<strong>$1</strong>")
    .replace(/(^|[^*])\*([^*\n]+)\*/g, "$1<em>$2</em>");

export function md(src) {
  if (!src) return "";
  let s = escH(String(src).replace(/\r/g, ""));
  const keep = [];
  const hold = (html) => `\n\n\u0000${keep.push(html) - 1}\u0000\n\n`;
  s = s.replace(/```[\w-]*\n([\s\S]*?)```/g, (_, code) => hold(`<pre><code>${code.replace(/\n$/, "")}</code></pre>`));
  s = s.replace(/((?:^[ \t]*\|[^\n]+\|[ \t]*\n?)+)/gm, (blk) => {
    const rows = blk.trim().split("\n").map((r) => r.trim()).filter(Boolean);
    if (rows.length < 2) return blk;
    let head = true, t = '<div class="tw"><table>';
    for (const r of rows) {
      if (/^\|[-:\s|]+\|$/.test(r)) { head = false; continue; }
      const cells = r.split("|").slice(1, -1).map((c) => inline(c.trim()));
      t += "<tr>" + cells.map((c) => (head ? `<th>${c}</th>` : `<td>${c}</td>`)).join("") + "</tr>";
      head = false;
    }
    return hold(t + "</table></div>");
  });
  s = s.replace(/((?:^[ \t]*(?:[-*]|\d+\.)\s+[^\n]+\n?)+)/gm, (blk) => {
    const ordered = /^\s*\d+\./.test(blk);
    const items = blk.trim().split("\n").map((l) => `<li>${inline(l.replace(/^[ \t]*(?:[-*]|\d+\.)\s+/, ""))}</li>`);
    return hold(`<${ordered ? "ol" : "ul"}>${items.join("")}</${ordered ? "ol" : "ul"}>`);
  });
  s = s.replace(/^#{1,2} (.*)$/gm, (_, h) => hold(`<h3>${inline(h)}</h3>`));
  s = s.replace(/^#{3,6} (.*)$/gm, (_, h) => hold(`<h4>${inline(h)}</h4>`));
  s = s.replace(/^\s*---+\s*$/gm, () => hold("<hr/>"));
  return s
    .split(/\n{2,}/)
    .map((p) => p.trim())
    .filter(Boolean)
    .map((p) => (/^\u0000\d+\u0000$/.test(p) ? p : `<p>${inline(p).replace(/\n/g, "<br/>")}</p>`))
    .join("")
    .replace(/\u0000(\d+)\u0000/g, (_, i) => keep[+i]);
}
