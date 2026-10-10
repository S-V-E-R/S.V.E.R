"use client";
import Link from "next/link";
import { FormEvent, useCallback, useState } from "react";
import { Section } from "../../../components/Form";
import { send, useLoad, type Result } from "../../../lib/client-api";

type Choice = { id: string; name: string; givable?: boolean };
type Server = { guild_name: string; post_channel: string | null; sub_role: string | null; guild_role: string | null; faction_roles: Record<string, string>; problem: string | null; synced_at: string | null; channels: Choice[]; roles: Choice[] };
type View = { available: boolean; server: Server | null };
const FACTIONS = [["myria", "Myria"], ["aetheron", "Aetheron"], ["glint", "Glint"]] as const;

/** Creator Studio → Discord (docs/COMMUNITY.md "Discord bot"): go-live posts and synced roles. */
export default function DiscordPage() {
  const [view, setView] = useState<View | null>(null);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const apply = (result: Result<View>, done: string) => { if (result.ok) { setView(result.data); setMessage(done); } else setMessage(result.error); };
  const load = useCallback(async () => {
    const result = await send<View>("GET", "/api/me/discord");
    const query = new URLSearchParams(window.location.search);
    if (result.ok) setView(result.data);
    setMessage(query.get("error") ?? (query.has("linked") ? "The bot joined your server. Pick where it posts and which roles it keeps in sync." : result.ok ? "" : result.error));
  }, []);
  useLoad(load);
  async function install() {
    setBusy(true);
    const result = await send<{ url: string }>("POST", "/api/me/discord/install");
    if (result.ok) window.location.assign(result.data.url); else { setMessage(result.error); setBusy(false); }
  }
  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const pick = (name: string) => (form.get(name) as string) || null;
    const faction_roles = Object.fromEntries(FACTIONS.map(([slug]) => [slug, pick(`faction-${slug}`)]).filter(([, role]) => role));
    setBusy(true);
    apply(await send<View>("PUT", "/api/me/discord", { post_channel: pick("post"), sub_role: pick("sub"), guild_role: pick("guild"), faction_roles }), "Saved. Roles sync within a minute, then every 15 minutes.");
    setBusy(false);
  }
  async function test() {
    const result = await send("POST", "/api/me/discord/test");
    setMessage(result.ok ? "Test post sent. Check your Discord channel." : result.error);
  }
  async function remove() {
    if (!window.confirm("Remove the bot? It takes back the roles it gave and leaves your server.")) return;
    apply(await send<View>("DELETE", "/api/me/discord"), "The bot left your server.");
  }
  if (!view) return message ? <p role="alert" className="form-message error">{message}</p> : <p className="loading">Loading…</p>;
  const s = view.server;
  const roleSelect = (name: string, label: string, value: string | null | undefined) => <label className="field narrow" key={name}><span>{label}</span>
    <select name={name} defaultValue={value ?? ""}>
      <option value="">None</option>
      {s?.roles.map(r => <option key={r.id} value={r.id} disabled={!r.givable}>{r.name}{!r.givable && " (above the S.V.E.R role)"}</option>)}
    </select></label>;
  return <><h1>Discord</h1>
    {message && <p role="status" className="form-message">{message}</p>}
    {!s ? <Section title="Add the S.V.E.R bot to your Discord server" intro="It posts in a channel you choose when you go live, and gives roles to your subscribers, to each faction and to your guild's members, taking them back when they no longer apply.">
      {view.available
        ? <><button type="button" onClick={install} disabled={busy}>Add to Discord</button>
          <p className="muted small">Discord asks you to pick a server you manage. The bot asks only to see channels, send messages and manage roles.</p></>
        : <p className="muted">Coming soon.</p>}
    </Section> : <Section title={`Connected to ${s.guild_name || "your server"}`} intro="Pick roles from your own server. The bot only takes back roles it gave, so roles your moderators hand out stay as they are.">
      {s.problem && <p role="alert" className="form-message error">{s.problem}</p>}
      <form onSubmit={save} className="stack">
        <label className="field narrow"><span>Post when I go live in</span>
          <select name="post" defaultValue={s.post_channel ?? ""}>
            <option value="">Don&apos;t post</option>
            {s.channels.map(c => <option key={c.id} value={c.id}>#{c.name}</option>)}
          </select></label>
        {roleSelect("sub", "Role for my subscribers", s.sub_role)}
        {FACTIONS.map(([slug, name]) => roleSelect(`faction-${slug}`, `Role for ${name}`, s.faction_roles[slug]))}
        {roleSelect("guild", "Role for my guild's members", s.guild_role)}
        <div className="row wrap">
          <button type="submit" className="small" disabled={busy}>Save</button>
          <button type="button" className="small quiet" onClick={test} disabled={!s.post_channel}>Send a test post</button>
          <button type="button" className="small quiet danger-text" onClick={remove}>Remove the bot</button>
        </div>
      </form>
      <p className="muted small">People get roles once they link their Discord account to S.V.E.R in <Link href="/account">Account</Link> (sign-in methods). If a role can&apos;t be given, open Server Settings → Roles in Discord and drag the S.V.E.R role above it.{s.synced_at && ` Last synced ${new Date(s.synced_at).toLocaleString()}.`}</p>
    </Section>}
  </>;
}
