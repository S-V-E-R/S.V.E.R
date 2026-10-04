import Link from "next/link";
import { notFound } from "next/navigation";
import { ChannelFrame } from "../../../components/ChannelFrame";
import { CopyButton } from "../../../components/CopyButton";
import { Markdown } from "../../../components/Markdown";
import { ReportButton } from "../../../components/Report";
import { channelMetadata, loadChannel, type ChannelParams } from "../../../lib/channel";
import { apiGet } from "../../../lib/server-api";
import { groupByCategory, kindLabel, setupLabel } from "../../../lib/setup-parts";

type Block = { type: "ABOUT" | "PANEL" | "QUOTES" | "GAME_SHELF"; enabled: boolean; config: { body?: string; title?: string; quotes?: { text: string; attribution: string }[]; games?: string[] } };
type Sponsor = { id: string; name: string; description: string; link: string; discount_code: string; category: string; logo: string | null };
type Setup = { category: string; name: string; note: string; link: string | null; kind?: string | null };
type SetupPhoto = { id: string; image: { "400": string; "1600": string }; alt: string };
type About = { bio: string; blocks: Block[]; sponsors: Sponsor[]; setup: Setup[]; setup_title?: string; setup_description?: string; setup_photos?: SetupPhoto[]; viewer?: { can_report: boolean } };
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
        <div><a href={s.link} rel="sponsored nofollow noopener noreferrer" target="_blank"><strong>{s.name}</strong></a><small className="muted">{label(s.category)}</small>{s.description && <p>{s.description}</p>}{s.discount_code && <p className="row tight">Code: <code>{s.discount_code}</code><CopyButton value={s.discount_code} label={`${s.name} discount code`} /></p>}</div>
      </li>)}</ul> : <p className="muted">Add sponsors in <Link href="/studio/channel/sponsors">Creator Studio</Link>.</p>}
    </section>}
    {(about?.setup.length || about?.setup_photos?.length || about?.setup_title || about?.setup_description || owner) && <section className="panel section setup-section"><h2>{about?.setup_title || "Streaming setup"}</h2>
      {about?.setup_title && <p className="eyebrow">STREAMING SETUP</p>}
      {about?.setup_description && <p className="setup-description">{about.setup_description}</p>}
      {!!about?.setup_photos?.length && <ul className="setup-photos">{about.setup_photos.map((p, i) => <li key={p.id}>
        {/* eslint-disable-next-line @next/next/no-img-element */}
        <a href={p.image["1600"]} target="_blank" rel="noopener"><img src={p.image["400"]} alt={p.alt || `Setup photo ${i + 1}`} loading="lazy" /></a>
        {about.viewer?.can_report && <ReportButton target={{ target_type: "setup_photo", target_id: p.id }} />}
      </li>)}</ul>}
      {about?.setup.length ? <dl className="setup">{groupByCategory(about.setup).map(([category, items]) => <div key={category}><dt>{setupLabel(category)}</dt>{items.map((s, i) => <dd key={i}>{s.link ? <a href={s.link} rel="sponsored nofollow noopener noreferrer" target="_blank">{s.name}</a> : s.name}{kindLabel(s.kind) && <small className="muted"> · {kindLabel(s.kind)}</small>}{s.note && <small className="muted"> — {s.note}</small>}</dd>)}</div>)}</dl> : (owner && <p className="muted">List your gear in <Link href="/studio/channel/setup">Creator Studio</Link>.</p>)}
    </section>}
  </ChannelFrame>;
}
