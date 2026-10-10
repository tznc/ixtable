/**
 * Rich-text fields store a small HTML subset. Everything is sanitized on the way in
 * (editor output) and again before display, because a value can also arrive from an
 * import, an action, or another PostgreSQL client.
 */
const ALLOWED = new Set([
  "P",
  "BR",
  "DIV",
  "B",
  "STRONG",
  "I",
  "EM",
  "U",
  "S",
  "UL",
  "OL",
  "LI",
  "A",
  "H1",
  "H2",
  "H3",
  "BLOCKQUOTE",
]);
/** Elements whose content is dropped with them, not kept as text. */
const DROPPED = new Set(["SCRIPT", "STYLE", "TEMPLATE", "IFRAME", "OBJECT", "EMBED", "NOSCRIPT"]);
const SAFE_HREF = /^(https?:|mailto:)/i;

function clean(source: Node, target: Node, doc: Document) {
  for (const node of [...source.childNodes]) {
    if (node.nodeType === Node.TEXT_NODE) {
      target.appendChild(doc.createTextNode(node.textContent ?? ""));
      continue;
    }
    if (node.nodeType !== Node.ELEMENT_NODE) continue;
    const element = node as Element;
    const tag = element.tagName.toUpperCase();
    if (DROPPED.has(tag)) continue;
    if (!ALLOWED.has(tag)) {
      clean(element, target, doc);
      continue;
    }
    const copy = doc.createElement(tag.toLowerCase());
    const href = tag === "A" ? (element.getAttribute("href") ?? "").trim() : "";
    if (SAFE_HREF.test(href)) {
      copy.setAttribute("href", href);
      copy.setAttribute("rel", "noopener noreferrer");
      copy.setAttribute("target", "_blank");
    }
    clean(element, copy, doc);
    target.appendChild(copy);
  }
}

/** The allowed subset of `html`: formatting tags only, links limited to http(s) and mailto. */
export function sanitizeRichText(html: string): string {
  if (!html) return "";
  const parsed = new DOMParser().parseFromString(`<body>${html}</body>`, "text/html");
  const out = document.implementation.createHTMLDocument("");
  const root = out.createElement("div");
  clean(parsed.body, root, out);
  return root.innerHTML;
}

const BLOCKS = /^(P|DIV|LI|H1|H2|H3|BLOCKQUOTE|BR)$/;

/** Plain text of a rich-text value, for lists, datasheets, reports, and validation. */
export function richTextPlain(html: unknown): string {
  if (html === null || html === undefined) return "";
  const text = String(html);
  if (!/[<&]/.test(text)) return text;
  const parsed = new DOMParser().parseFromString(`<body>${text}</body>`, "text/html");
  const parts: string[] = [];
  const walk = (node: Node) => {
    for (const child of node.childNodes) {
      if (child.nodeType === Node.TEXT_NODE) parts.push(child.textContent ?? "");
      else if (child.nodeType === Node.ELEMENT_NODE) {
        const tag = (child as Element).tagName.toUpperCase();
        if (DROPPED.has(tag)) continue;
        if (BLOCKS.test(tag) && parts.length) parts.push("\n");
        walk(child);
      }
    }
  };
  walk(parsed.body);
  return parts
    .join("")
    .replace(/\n{2,}/g, "\n")
    .trim();
}
