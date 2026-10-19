"use client";
import { FormEvent, useCallback, useState } from "react";
import { send, useLoad } from "../../../lib/client-api";

type Field = { key: string; kind: "text" | "forum" | "voice" | "category" | "texts" | "given" | "role" | "toggle" | "zone" | "number" | "words" | "roles" | "optin"; group: string; label: string };
type Channel = { id: string; name: string; type: number; parent: string | null };
type Role = { id: string; name: string; givable: boolean };
type Health = {
  enabled: boolean;
  gateway: { connected: boolean; connected_at: string | null; last_event_at: string | null; problem: string | null; bot_name: string | null } | null;
  home?: { guild_id: string; missing?: string[]; roles_above_bot?: string[]; error?: string };
};
type View = {
  health: Health;
  guilds: { id: string; name: string }[];
  fields: Field[];
  home: { guild_id: string; settings: Record<string, unknown>; channels: Channel[]; roles: Role[] } | null;
};

/** Discord channel types each kind of setting accepts (matches discord/home.rs). */
const TYPES: Record<string, number[]> = { text: [0, 5], texts: [0, 5], forum: [0, 5, 15], voice: [2, 13], category: [4] };
const when = (t: string | null) => (t ? new Date(t).toLocaleString() : "never");

/** The S.V.E.R home server's Discord bot (docs/DISCORD_BOT.md): health and settings. */
export default function DiscordAdmin() {
  const [view, setView] = useState<View | null>(null);
  const [guild, setGuild] = useState("");
  const [message, setMessage] = useState<{ error?: string; saved?: string; field?: string }>({});
  const load = useCallback(async () => {
    const r = await send<View>("GET", "/api/admin/discord");
    if (r.ok) { setView(r.data); setGuild(r.data.home?.guild_id ?? ""); } else setMessage({ error: r.error });
  }, []);
  useLoad(load);

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!view) return;
    const form = new FormData(event.currentTarget);
    const settings: Record<string, unknown> = {};
    // The fields show once the server's channels and roles are loaded; before that, save the server alone.
    const shown = view.home && view.home.guild_id === guild;
    for (const f of shown ? view.fields : []) {
      if (f.kind === "toggle" || f.kind === "optin") settings[f.key] = form.get(f.key) === "on";
      else if (f.kind === "texts" || f.kind === "roles") settings[f.key] = form.getAll(f.key).map(String).filter(Boolean);
      else if (f.kind === "words") settings[f.key] = String(form.get(f.key) ?? "").split("\n").map(w => w.trim()).filter(Boolean);
      else if (f.kind === "number") { const v = String(form.get(f.key) ?? "").trim(); settings[f.key] = v ? Number(v) : null; }
      else { const v = String(form.get(f.key) ?? ""); settings[f.key] = v || null; }
    }
    const r = await send<View>("PUT", "/api/admin/discord", { guild_id: guild, settings });
    if (r.ok) { setView(r.data); setMessage({ saved: "Saved. The bot's commands follow the home server." }); } else setMessage({ error: r.error, field: r.field });
  }

  if (!view) return <section className="panel section"><h1>Discord bot</h1>{message.error ? <p role="alert" className="form-message error">{message.error}</p> : <p className="loading">Loading…</p>}</section>;
  const h = view.health;
  const home = view.home && view.home.guild_id === guild ? view.home : null;
  const groups = [...new Set(view.fields.map(f => f.group))];
  const label = (f: Field) => `${f.label}${message.field === f.key ? " (check this)" : ""}`;
  const input = (f: Field) => {
    const value = home?.settings[f.key];
    if (f.kind === "toggle" || f.kind === "optin") return <label key={f.key} className="field checkbox"><input type="checkbox" name={f.key} defaultChecked={f.kind === "optin" ? value === true : value !== false} /> <span>{label(f)}</span></label>;
    if (f.kind === "number") return <label key={f.key} className="field narrow"><span>{label(f)}</span><input type="number" name={f.key} min={0} max={10000000} step={1} defaultValue={typeof value === "number" ? value : ""} placeholder="Off" /></label>;
    if (f.kind === "words") return <label key={f.key} className="field"><span>{label(f)} (one per line)</span><textarea name={f.key} rows={4} defaultValue={Array.isArray(value) ? value.map(String).join("\n") : ""} /></label>;
    if (f.kind === "roles") {
      const picked = Array.isArray(value) ? value.map(String) : [];
      return <label key={f.key} className="field"><span>{label(f)}</span>
        <select name={f.key} multiple size={Math.min(6, Math.max(2, home?.roles.length ?? 2))} defaultValue={picked}>{(home?.roles ?? []).map(r => <option key={r.id} value={r.id}>{r.name}</option>)}</select></label>;
    }
    if (f.kind === "zone") return <label key={f.key} className="field narrow"><span>{label(f)}</span><input name={f.key} list="zones" defaultValue={typeof value === "string" ? value : ""} placeholder="America/New_York" /></label>;
    if (f.kind === "given" || f.kind === "role") return <label key={f.key} className="field narrow"><span>{label(f)}</span>
      <select name={f.key} defaultValue={typeof value === "string" ? value : ""}><option value="">None</option>
        {(home?.roles ?? []).map(r => <option key={r.id} value={r.id} disabled={f.kind === "given" && !r.givable}>{r.name}{f.kind === "given" && !r.givable ? " (above the bot)" : ""}</option>)}
      </select></label>;
    const channels = (home?.channels ?? []).filter(c => TYPES[f.kind]?.includes(c.type));
    if (f.kind === "texts") {
      const picked = Array.isArray(value) ? value.map(String) : [];
      return <label key={f.key} className="field"><span>{label(f)}</span>
        <select name={f.key} multiple size={Math.min(6, Math.max(2, channels.length))} defaultValue={picked}>{channels.map(c => <option key={c.id} value={c.id}>#{c.name}</option>)}</select></label>;
    }
    return <label key={f.key} className="field narrow"><span>{label(f)}</span>
      <select name={f.key} defaultValue={typeof value === "string" ? value : ""}><option value="">None</option>{channels.map(c => <option key={c.id} value={c.id}>#{c.name}</option>)}</select></label>;
  };

  return <section className="panel section"><h1>Discord bot</h1>
    <p className="intro">The community bot on the S.V.E.R Discord server. Pick the server, then its channels and roles. Roles the bot gives must sit below its own role in Discord (Server Settings → Roles). Automod and safety numbers and word lists are private tuning: an empty one turns its check off.</p>
    <h2>Health</h2>
    <ul className="list">
      <li><strong>Connection:</strong> {!h.enabled ? "Off on this server (DISCORD_GATEWAY isn't set)." : h.gateway?.connected ? `Connected as ${h.gateway.bot_name ?? "the bot"} since ${when(h.gateway.connected_at)}.` : `Not connected.${h.gateway?.problem ? ` ${h.gateway.problem}` : ""}`}</li>
      <li><strong>Last event:</strong> {when(h.gateway?.last_event_at ?? null)}</li>
      {h.home?.error && <li role="alert" className="error">{h.home.error}</li>}
      {h.home?.missing && <li><strong>Missing permissions:</strong> {h.home.missing.length ? h.home.missing.join(", ") : "none"}</li>}
      {h.home?.roles_above_bot && h.home.roles_above_bot.length > 0 && <li role="alert" className="error"><strong>Roles above the bot:</strong> {h.home.roles_above_bot.join(", ")}</li>}
    </ul>
    <form className="stack" onSubmit={e => void save(e)}>
      <label className="field narrow"><span>Home server{message.field === "guild_id" ? " (check this)" : ""}</span>
        <select value={guild} onChange={e => setGuild(e.target.value)} required><option value="">Choose a server</option>{view.guilds.map(g => <option key={g.id} value={g.id}>{g.name}</option>)}</select></label>
      {view.guilds.length === 0 && <p className="muted">The bot hasn&apos;t seen any server yet. Add it to the S.V.E.R server, and make sure it&apos;s connected.</p>}
      {guild && !home && <p className="muted">Save to load this server&apos;s channels and roles.</p>}
      {home && groups.map(g => <fieldset key={g} className="stack"><legend>{g}</legend><div className="row wrap">{view.fields.filter(f => f.group === g).map(input)}</div></fieldset>)}
      <datalist id="zones">{(typeof Intl.supportedValuesOf === "function" ? Intl.supportedValuesOf("timeZone") : []).map(z => <option key={z} value={z} />)}</datalist>
      {message.error && <p role="alert" className="form-message error">{message.error}</p>}
      {message.saved && <p role="status" className="form-message">{message.saved}</p>}
      <span className="row wrap"><button type="submit" disabled={!guild}>Save</button></span>
    </form>
  </section>;
}
