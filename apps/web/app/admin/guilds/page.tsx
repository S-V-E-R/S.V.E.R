"use client";
import Link from "next/link";
import { useCallback, useState, type FormEvent } from "react";
import { send, useLoad } from "../../../lib/client-api";
import type { Sizes } from "../../../lib/types";
import "../../../styles/teams.css";
type Item = { id: string; name: string; slug: string; tag: string; status: string; verified: boolean; emblem_pending: boolean; verification_status: string | null; evidence: string | null; note: string | null; content: { image: string | null; banner: Sizes; tagline: string; about: string } };
export default function GuildReview() {
  const [items, setItems] = useState<Item[] | null>(null);
  const [query, setQuery] = useState("");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const load = useCallback(async () => { const r = await send<{ items: Item[] }>("GET", `/api/admin/guilds?q=${encodeURIComponent(query)}`); if (r.ok) setItems(r.data.items); else setNotice(r.error); }, [query]);
  useLoad(load);
  async function act(event: FormEvent<HTMLFormElement>, guild: Item) {
    event.preventDefault(); const data = new FormData(event.currentTarget), action = data.get("action");
    if ((action === "disband" || action === "reset") && !window.confirm(`${action === "disband" ? "Disband" : "Reset branding for"} ${guild.name}?`)) return;
    setBusy(true); const r = await send("POST", `/api/admin/guilds/${guild.id}`, { action, note: data.get("note"), ...(action === "rename" ? { branding: { name: data.get("name"), slug: data.get("slug"), tag: data.get("tag") } } : {}) });
    setNotice(r.ok ? "Action recorded." : r.error); if (r.ok) await load(); setBusy(false);
  }
  return <><h1>Guild review</h1><p>New emblems and organization requests. Search to review any guild.</p><form onSubmit={e => { e.preventDefault(); setQuery(String(new FormData(e.currentTarget).get("q") ?? "")); }}><label className="field">Guild name or tag<input type="search" name="q" maxLength={100} /></label><button>Search</button></form>
    {notice && <p role="status" className="form-message">{notice}</p>}
    {!items ? <p>Loading…</p> : items.length === 0 ? <p>No guilds to review.</p> : items.map(g => <section key={g.id} className="panel section"><h2><Link href={`/g/${g.slug}`}>{g.name}</Link> [{g.tag}]</h2><p>{g.status.toLowerCase()}{g.verified && " · Verified organization"}</p>
      {/* eslint-disable-next-line @next/next/no-img-element */}
      {g.content.image && <img src={g.content.image} width={112} height={112} alt={`${g.name} emblem`} />}{g.emblem_pending && <p>Emblem awaiting review. Check for original artwork and imitation of site, faction or verified marks.</p>}
      {/* eslint-disable-next-line @next/next/no-img-element */}
      {g.content.banner && <img className="guild-banner" src={Object.values(g.content.banner).at(-1)} alt={`${g.name} banner`} />}<p>{g.content.tagline}</p><p className="guild-copy">{g.content.about}</p>{g.evidence && <><h3>Organization evidence · {g.verification_status?.toLowerCase()}</h3><p className="guild-copy">{g.evidence}</p><p>{g.note}</p></>}
      <form onSubmit={e => act(e, g)}><label className="field">Action<select name="action" required><option value="approve_emblem">Approve emblem</option><option value="remove_emblem">Remove emblem</option><option value="verify">Verify organization</option><option value="decline_verification">Decline organization request</option><option value="unverify">Remove verification</option><option value="rename">Rename identity</option><option value="reset">Reset text and images</option><option value="disband">Disband guild</option></select></label><details><summary>Replacement identity for Rename</summary><label className="field">Name<input name="name" defaultValue={g.name} minLength={3} maxLength={40} /></label><label className="field">URL name<input name="slug" defaultValue={g.slug} maxLength={25} /></label><label className="field">Tag<input name="tag" defaultValue={g.tag} maxLength={5} /></label></details><label className="field">Staff note<textarea name="note" required maxLength={500} rows={3} /></label><button disabled={busy}>Record action</button></form>
    </section>)}
  </>;
}
