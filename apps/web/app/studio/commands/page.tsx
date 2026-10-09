"use client";
import { FormEvent, useCallback, useState } from "react";
import { Section } from "../../../components/Form";
import { send, useLoad, type Result } from "../../../lib/client-api";

type Command = { name: string; reply: string; access: string; cooldown_seconds: number; uses: number };
type Timer = { id: string; body: string; every_minutes: number; enabled: boolean; last_sent_at: string | null };
type Data = { commands: Command[]; timers: Timer[]; bot: { key: string; name: string; chosen: string | null; personality: string }; max_timers: number };

const BOTS: Record<string, string> = { pyre: "PYRE (Myria)", echo: "ECHO (Aetheron)", favor: "FAVOR (Glint)", volk: "VOLK (neutral)" };
const ACCESS: Record<string, string> = { everyone: "Everyone", followers: "Followers", subscribers: "Subscribers", moderators: "Moderators" };

/** Creator Studio → Commands & bot (docs/COMMUNITY.md "Chat commands and the channel bot"). */
export default function CommandsPage() {
  const [data, setData] = useState<Data | null>(null);
  const [message, setMessage] = useState("");
  const apply = (result: Result<Data>, saved: string) => {
    if (result.ok) { setData(result.data); setMessage(saved); } else setMessage(result.error);
  };
  const load = useCallback(async () => {
    const result = await send<Data>("GET", "/api/me/commands");
    if (result.ok) setData(result.data); else setMessage(result.error);
  }, []);
  useLoad(load);
  async function saveCommand(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    const v = Object.fromEntries(new FormData(form)) as Record<string, string>;
    const name = v.name.trim().replace(/^!/, "");
    const result = await send<Data>("PUT", `/api/me/commands/${encodeURIComponent(name)}`, { reply: v.reply, access: v.access, cooldown_seconds: Number(v.cooldown) || 0 });
    apply(result, `!${name.toLowerCase()} saved.`);
    if (result.ok) form.reset();
  }
  async function addTimer(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    const v = Object.fromEntries(new FormData(form)) as Record<string, string>;
    const result = await send<Data>("POST", "/api/me/timers", { body: v.body, every_minutes: Number(v.every) });
    apply(result, "Timed message added.");
    if (result.ok) form.reset();
  }
  if (!data) return message ? <p role="alert" className="form-message error">{message}</p> : <p className="loading">Loading…</p>;
  return <><h1>Commands &amp; bot</h1>
    {message && <p role="status" className="form-message">{message}</p>}
    <Section title="Your channel's bot" intro="Command replies and timed messages come from your bot, shown in chat with a Bot badge. By default it's your faction's bot; VOLK is the neutral one.">
      <div className="row wrap">
        <label className="field narrow"><span>Bot</span><select value={data.bot.chosen ?? ""} onChange={async e => apply(await send<Data>("PUT", "/api/me/commands/bot", { bot: e.target.value || null, personality: data.bot.personality }), "Bot saved.")}>
          <option value="">My faction&apos;s bot{data.bot.chosen ? "" : ` (${data.bot.name})`}</option>
          {Object.entries(BOTS).map(([k, n]) => <option key={k} value={k}>{n}</option>)}
        </select></label>
        <label className="field narrow"><span>Personality</span><select value={data.bot.personality} onChange={async e => apply(await send<Data>("PUT", "/api/me/commands/bot", { bot: data.bot.chosen, personality: e.target.value }), "Personality saved.")}>
          <option value="chill">Chill (warm, little faction flavor)</option><option value="battle">Battle (full faction personality)</option><option value="event">Event (maximum hype)</option>
        </select></label>
      </div>
    </Section>
    <Section title="Custom commands" intro="Viewers type !name and your bot replies. Variables: {user}, {channel}, {uptime}, {game}, {followers}. Replies follow your chat's banned words and link rule. Viewers can type /help to see them.">
      {data.commands.length > 0 && <ul className="list">{data.commands.map(c => <li key={c.name} className="row between wrap">
        <span><strong>!{c.name}</strong> <span className="muted small">{ACCESS[c.access]} · {c.cooldown_seconds}s cooldown · used {c.uses.toLocaleString()} times</span><br />{c.reply}</span>
        <button type="button" className="small quiet danger-text" onClick={async () => apply(await send<Data>("DELETE", `/api/me/commands/${c.name}`), `!${c.name} deleted.`)}>Delete</button>
      </li>)}</ul>}
      <form className="stack" onSubmit={saveCommand}>
        <div className="row wrap">
          <label className="field narrow"><span>Command</span><input name="name" required maxLength={26} placeholder="!discord" autoComplete="off" /></label>
          <label className="field narrow"><span>Who can use it</span><select name="access" defaultValue="everyone">{Object.entries(ACCESS).map(([k, n]) => <option key={k} value={k}>{n}</option>)}</select></label>
          <label className="field narrow"><span>Cooldown (seconds)</span><input name="cooldown" type="number" min={0} max={3600} defaultValue={10} /></label>
        </div>
        <label className="field"><span>Reply (up to 300 characters)</span><textarea name="reply" required maxLength={300} rows={2} /></label>
        <button type="submit">Save command</button>
        <small className="muted">Saving an existing name changes it.</small>
      </form>
    </Section>
    <Section title="Timed messages" intro={`Up to ${data.max_timers} messages your bot posts every N minutes (at least 10) while you're live, and only when chat has been active since the last one.`}>
      {data.timers.length > 0 && <ul className="list">{data.timers.map(t => <li key={t.id} className="row between wrap">
        <span>{t.body} <span className="muted small">every {t.every_minutes} min{t.last_sent_at ? ` · last posted ${new Date(t.last_sent_at).toLocaleTimeString()}` : ""}</span></span>
        <span className="row">
          <label className="row"><input type="checkbox" checked={t.enabled} onChange={async e => apply(await send<Data>("PATCH", `/api/me/timers/${t.id}`, { enabled: e.target.checked }), e.target.checked ? "Timer on." : "Timer off.")} /> On</label>
          <button type="button" className="small quiet danger-text" onClick={async () => apply(await send<Data>("DELETE", `/api/me/timers/${t.id}`), "Timed message deleted.")}>Delete</button>
        </span>
      </li>)}</ul>}
      {data.timers.length < data.max_timers && <>
        <form className="stack" onSubmit={addTimer}>
          <label className="field"><span>Message</span><textarea name="body" required maxLength={300} rows={2} /></label>
          <label className="field narrow"><span>Every (minutes)</span><input name="every" type="number" min={10} max={1440} defaultValue={30} required /></label>
          <button type="submit">Add timed message</button>
        </form>
        {data.timers.length === 0 && <button type="button" className="quiet small" onClick={async () => apply(await send<Data>("POST", "/api/me/timers/starter"), "Starter messages added.")}>Add starter messages (chat rules and your social links)</button>}
      </>}
    </Section>
  </>;
}
