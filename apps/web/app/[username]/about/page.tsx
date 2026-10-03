import Link from "next/link";
import { notFound } from "next/navigation";
import { ChannelFrame } from "../../../components/ChannelFrame";
import { Markdown } from "../../../components/Markdown";
import { channelMetadata, loadChannel, type ChannelParams } from "../../../lib/channel";
import { apiGet } from "../../../lib/server-api";

type Block = { type: "ABOUT" | "PANEL" | "QUOTES" | "GAME_SHELF"; enabled: boolean; config: { body?: string; title?: string; quotes?: { text: string; attribution: string }[]; games?: string[] } };
type Sponsor = { id: string; name: string; description: string; link: string; discount_code: string; category: string; logo: string | null };
type Setup = { category: string; name: string; note: string; link: string | null };
type About = { bio: string; blocks: Block[]; sponsors: Sponsor[]; setup: Setup[] };
const label = (value: string) => value.toLowerCase().replaceAll("_", " ").replace(/^./, c => c.toUpperCase());

export async function generateMetadata({ params }: { params: ChannelParams }) {
  return channelMetadata((await params).username, "About");
}
export default async function AboutTab({ params }: { params: ChannelParams }) {
  const data = await loadChannel((await params).username);
  if (!data.tabs.about) notFound();
  const name = data.channel.username;
  const about = (await apiGet<About>(`/api/channels/${encodeURIComponent(name)}/about`)).data;
  const owner = data.viewer.is_owner;
  return <ChannelFrame data={data} path={`/${name}/about`}>
    {about?.blocks.map((b, i) => <section key={i} className="panel section block">
      {b.type === "ABOUT" && <><h2>About</h2><Markdown text={b.config.body || ""} /></>}
      {b.type === "PANEL" && <><h2>{b.config.title}</h2><Markdown text={b.config.body || ""} /></>}
      {b.type === "QUOTES" && <><h2>Quotes</h2>{b.config.quotes?.map((q, j) => <blockquote key={j}><p>{q.text}</p>{q.attribution && <cite>— {q.attribution}</cite>}</blockquote>)}</>}
      {b.type === "GAME_SHELF" && <><h2>Game shelf</h2><ul className="tags">{b.config.games?.map(g => <li key={g}>{g}</li>)}</ul></>}
    </section>)}
    {owner && !about?.blocks.length && <p className="panel section muted">Add About blocks in <Link href="/studio/channel/blocks">Creator Studio</Link>.</p>}
    {(about?.sponsors.length || owner) && <section className="panel section"><h2>Sponsors</h2>
      {about?.sponsors.length ? <ul className="cards">{about.sponsors.map(s => <li key={s.id} className="panel">
        {/* eslint-disable-next-line @next/next/no-img-element */}
        {s.logo && <img src={s.logo} alt={`${s.name} logo`} width={64} height={64} />}
        <div><a href={s.link} rel="sponsored noopener noreferrer" target="_blank"><strong>{s.name}</strong></a><small className="muted">{label(s.category)}</small>{s.description && <p>{s.description}</p>}{s.discount_code && <p>Code: <code>{s.discount_code}</code></p>}</div>
      </li>)}</ul> : <p className="muted">Add sponsors in <Link href="/studio/channel/sponsors">Creator Studio</Link>.</p>}
    </section>}
    {(about?.setup.length || owner) && <section className="panel section"><h2>Streaming setup</h2>
      {about?.setup.length ? <dl className="setup">{about.setup.map((s, i) => <div key={i}><dt>{label(s.category)}</dt><dd>{s.link ? <a href={s.link} rel="nofollow noopener noreferrer ugc" target="_blank">{s.name}</a> : s.name}{s.note && <small className="muted"> — {s.note}</small>}</dd></div>)}</dl> : <p className="muted">List your gear in <Link href="/studio/channel/setup">Creator Studio</Link>.</p>}
    </section>}
  </ChannelFrame>;
}
