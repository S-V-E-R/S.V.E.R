import { Fragment, type ReactNode } from "react";

// The reduced Markdown subset for About and Panel blocks: paragraphs, "- " lists, **bold**,
// *italic* and [text](https://...) links. Everything is rendered as text; no HTML passes through.
function inline(text: string): ReactNode[] {
  const out: ReactNode[] = [];
  const pattern = /\*\*([^*]+)\*\*|\*([^*]+)\*|\[([^\]]+)\]\((https:\/\/[^\s)]+)\)/g;
  let last = 0;
  for (const m of text.matchAll(pattern)) {
    out.push(text.slice(last, m.index));
    if (m[1]) out.push(<strong key={m.index}>{m[1]}</strong>);
    else if (m[2]) out.push(<em key={m.index}>{m[2]}</em>);
    else out.push(<a key={m.index} href={m[4]} rel="nofollow noopener noreferrer ugc" target="_blank">{m[3]}</a>);
    last = (m.index ?? 0) + m[0].length;
  }
  out.push(text.slice(last));
  return out;
}
export function Markdown({ text }: { text: string }) {
  return <div className="markdown">{text.split(/\n{2,}/).map((block, i) => {
    const lines = block.split("\n");
    if (lines.every(l => l.startsWith("- "))) return <ul key={i}>{lines.map((l, j) => <li key={j}>{inline(l.slice(2))}</li>)}</ul>;
    return <p key={i}>{lines.map((l, j) => <Fragment key={j}>{j > 0 && <br />}{inline(l)}</Fragment>)}</p>;
  })}</div>;
}
