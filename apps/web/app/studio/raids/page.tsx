"use client";
import Link from "next/link";
import { FormEvent, useCallback, useState } from "react";
import { Section } from "../../../components/Form";
import { send, useLoad, type Result } from "../../../lib/client-api";

type Raid = { id: string; status: string; execute_at: string; target: { username: string; display_name: string } };
type Hosting = { accept_raids: boolean; accept_hosts: boolean; auto_host: boolean; auto_list: string[]; raid_blocks: string[]; live: boolean; hosting: { username: string; display_name: string; source: "manual" | "auto" | "raid" } | null; raid: Raid | null };

const names = (text: string) => text.split(/[\s,]+/).map(n => n.replace(/^@/, "")).filter(Boolean);

/** Creator Studio → Raids & hosting (docs/LIVE_STREAMS.md "Raids" and "Hosting"). */
export default function RaidsAndHosting() {
  const [data, setData] = useState<Hosting | null>(null);
  const [message, setMessage] = useState("");
  const [list, setList] = useState("");
  const apply = (result: Result<Hosting>, saved = "Saved.") => {
    if (result.ok) { setData(result.data); setList(result.data.auto_list.join(", ")); setMessage(saved); } else setMessage(result.error);
  };
  const load = useCallback(async () => {
    const result = await send<Hosting>("GET", "/api/me/hosting");
    if (result.ok) { setData(result.data); setList(result.data.auto_list.join(", ")); } else setMessage(result.error);
  }, []);
  useLoad(load);
  async function save(change: Partial<Hosting>) {
    if (!data) return;
    const next = { ...data, ...change };
    apply(await send<Hosting>("PUT", "/api/me/hosting", { accept_raids: next.accept_raids, accept_hosts: next.accept_hosts, auto_host: next.auto_host, auto_list: names(list) }));
  }
  async function field(event: FormEvent<HTMLFormElement>, run: (value: string) => Promise<void>) {
    event.preventDefault();
    const form = event.currentTarget;
    const value = String(new FormData(form).get("username") ?? "").trim().replace(/^@/, "");
    if (!value) return;
    await run(value);
    form.reset();
  }
  async function raid(name: string) {
    const result = await send("POST", "/api/me/raids", { username: name });
    setMessage(result.ok ? `Raid on @${name} started.` : result.error);
    await load();
  }
  async function cancel() {
    const result = await send("DELETE", "/api/me/raids");
    setMessage(result.ok ? "Raid cancelled." : result.error);
    await load();
  }
  if (!data) return message ? <p role="alert" className="form-message error">{message}</p> : <p className="loading">Loading…</p>;
  return <><h1>Raids &amp; hosting</h1>
    <Section title="Raid" intro="While you're live, send your viewers to another live channel. They see a 10-second countdown and can choose to stay. You can raid once every 10 minutes. In your chat, /raid username works too, and /unraid cancels.">
      {data.raid ? <p role="status">Raiding <Link href={`/${data.raid.target.username}`}>{data.raid.target.display_name}</Link>{data.raid.status === "countdown" && <> <button type="button" className="small quiet" onClick={cancel}>Cancel raid</button></>}</p>
        : data.live ? <form className="row" onSubmit={e => field(e, raid)}>
          <label className="field"><span>Channel to raid</span><input name="username" autoComplete="off" required maxLength={26} /></label><button type="submit">Start raid</button>
        </form> : <p className="muted">Go live to start a raid.</p>}
    </Section>
    <Section title="Hosting" intro="While you're offline, your channel page can show another live channel. Hosting stops when you go live or they go offline. After you raid and end your stream, your channel hosts the channel you raided.">
      {data.hosting ? <p role="status">Hosting <Link href={`/${data.hosting.username}`}>{data.hosting.display_name}</Link> {data.hosting.source !== "manual" && <span className="muted">({data.hosting.source === "auto" ? "auto-host" : "after your raid"})</span>} <button type="button" className="small quiet" onClick={async () => apply(await send<Hosting>("PUT", "/api/me/hosting/target", { username: null }), "Stopped hosting.")}>Stop hosting</button></p>
        : <form className="row" onSubmit={e => field(e, async name => apply(await send<Hosting>("PUT", "/api/me/hosting/target", { username: name }), `Hosting @${name}.`))}>
          <label className="field"><span>Channel to host</span><input name="username" autoComplete="off" required maxLength={26} disabled={data.live} /></label><button type="submit" disabled={data.live}>Host</button>
        </form>}
      <label className="checkbox"><input type="checkbox" checked={data.auto_host} onChange={e => save({ auto_host: e.target.checked })} /> Auto-host: when I&apos;m offline, host the first live channel on my list</label>
      <form onSubmit={e => { e.preventDefault(); void save({}); }}>
        <label className="field"><span>Auto-host list, in priority order (up to 10)</span><input value={list} onChange={e => setList(e.target.value)} placeholder="username, username" /></label>
        <button type="submit" className="small">Save list</button>
      </form>
      {data.auto_host && <p className="muted">While auto-host is on, stopping a host lets it pick the next live channel. Turn auto-host off to stop it.</p>}
    </Section>
    <Section title="Incoming raids and hosts" intro="Channels you've blocked, or banned from your chat, can never raid or host you.">
      <label className="checkbox"><input type="checkbox" checked={data.accept_raids} onChange={e => save({ accept_raids: e.target.checked })} /> Accept raids</label>
      <label className="checkbox"><input type="checkbox" checked={data.accept_hosts} onChange={e => save({ accept_hosts: e.target.checked })} /> Allow other channels to host me</label>
      <form className="row" onSubmit={e => field(e, async name => apply(await send<Hosting>("POST", "/api/me/raid-blocks", { username: name }), `@${name} can't raid or host you now.`))}>
        <label className="field"><span>Block raids and hosts from a channel</span><input name="username" autoComplete="off" required maxLength={26} /></label><button type="submit" className="small">Block</button>
      </form>
      {data.raid_blocks.length > 0 && <ul className="list">{data.raid_blocks.map(name => <li key={name} className="row between"><Link href={`/${name}`}>@{name}</Link><button type="button" className="small quiet" onClick={async () => apply(await send<Hosting>("DELETE", `/api/me/raid-blocks/${encodeURIComponent(name)}`))}>Unblock</button></li>)}</ul>}
    </Section>
    {message && <p role="status" className="form-message">{message}</p>}
  </>;
}
