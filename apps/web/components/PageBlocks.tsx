import { Markdown } from "./Markdown";
import type { PageBlock } from "../lib/types";

/** About me: the owner's blocks in their order (docs/PROFILES.md "Custom page blocks", "Persona pass"). */
export function PageBlocks({ blocks }: { blocks: PageBlock[] }) {
  return <div className="page-blocks">{blocks.map((b, i) => <section key={i} className="panel section block" data-type={b.type.toLowerCase()}>
    {b.type === "ABOUT" && <><h2>About me</h2><Markdown text={b.config.body || ""} /></>}
    {b.type === "PANEL" && <><h2>{b.config.title}</h2><Markdown text={b.config.body || ""} /></>}
    {b.type === "QUOTES" && <><h2>Quotes</h2>{b.config.quotes?.map((q, j) => <blockquote key={j}><p>{q.text}</p>{q.attribution && <cite>— {q.attribution}</cite>}</blockquote>)}</>}
    {b.type === "GAME_SHELF" && <><h2>Game shelf</h2><ul className="tags">{b.config.games?.map(g => <li key={g}>{g}</li>)}</ul></>}
    {b.type === "PROMPT" && <><h2 className="prompt-question">{b.config.title}</h2><p className="prompt-answer">{b.config.answer}</p></>}
  </section>)}</div>;
}
