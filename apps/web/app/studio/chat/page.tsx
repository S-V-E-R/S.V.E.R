"use client";
import { FormEvent, useCallback, useState } from "react";
import { Section, Status, type SaveState } from "../../../components/Form";
import { UserChip } from "../../../components/UserChip";
import { send, useLoad } from "../../../lib/client-api";
import type { Chip } from "../../../lib/types";

type View = {
  role: string;
  settings: { slow_mode_seconds: number; block_links: boolean; banned_words: string[] };
  moderators: Chip[];
  restrictions: { user: Chip; kind: "timeout" | "ban"; until: string | null }[];
  log: { action: string; actor_role: string; actor: string | null; target: string | null; reason: string; created_at: string }[];
};
const label = (action: string) => action.replaceAll("_", " ");
const when = (iso: string) => new Date(iso).toLocaleString("en-US", { dateStyle: "medium", timeStyle: "short" });

/** Chat rules, moderators, active timeouts/bans and the audit log for the owner's channel. */
export default function ChatStudio() {
  const [username, setUsername] = useState("");
  const [view, setView] = useState<View | null>(null);
  const [words, setWords] = useState("");
  const [state, setState] = useState<SaveState>({});
  const [modState, setModState] = useState<SaveState>({});
  const base = username ? `/api/channels/${encodeURIComponent(username)}` : "";

  const load = useCallback(async () => {
    const me = await send<{ username: string }>("GET", "/api/auth/me");
    if (!me.ok) return;
    setUsername(me.data.username);
    const r = await send<View>("GET", `/api/channels/${encodeURIComponent(me.data.username)}/chat/moderation`);
    if (r.ok) { setView(r.data); setWords(r.data.settings.banned_words.join("\n")); }
  }, []);
  useLoad(load);
  if (!view) return <p className="loading">Loading…</p>;

  async function saveRules(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const result = await send("PUT", `${base}/chat/settings`, {
      slow_mode_seconds: Number(form.get("slow")), block_links: form.get("links") === "on",
      banned_words: words.split("\n").map(w => w.trim()).filter(Boolean), reason: "Updated in Creator Studio",
    });
    setState(result.ok ? { saved: "Chat rules saved." } : result);
    if (result.ok) load();
  }
  async function appoint(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const input = new FormData(event.currentTarget).get("username");
    const result = await send("POST", `${base}/moderators`, { username: input });
    setModState(result.ok ? { saved: "Moderator added." } : result.status === 403 && result.error.includes("Confirm") ? { error: "Confirm your sign-in method again on the Account page, then retry." } : result);
    if (result.ok) load();
  }
  async function dismiss(name: string) {
    if (!window.confirm(`Remove @${name} as a moderator?`)) return;
    const result = await send("DELETE", `${base}/moderators/${encodeURIComponent(name)}`);
    setModState(result.ok ? { saved: "Moderator removed." } : result);
    if (result.ok) load();
  }
  async function lift(name: string, kind: string) {
    const reason = window.prompt(`Reason for lifting this ${kind}`);
    if (!reason) return;
    const result = await send("DELETE", `${base}/chat/restrictions/${encodeURIComponent(name)}/${kind}`, { reason });
    setModState(result.ok ? { saved: kind === "ban" ? "Ban lifted." : "Timeout lifted." } : result);
    if (result.ok) load();
  }

  const s = view.settings;
  return <><h1>Chat</h1>
    <Section title="Rules" intro="Slow mode and the link rule don't apply to you or your moderators. Banned words apply to everyone.">
      <form onSubmit={saveRules} className="stack">
        <label className="field"><span>Slow mode (0 for off, or 3–120 seconds between messages)</span><input name="slow" type="number" min={0} max={120} defaultValue={s.slow_mode_seconds} /></label>
        <label className="row"><input name="links" type="checkbox" defaultChecked={s.block_links} /> Block links</label>
        <label className="field"><span>Banned words or phrases (one per line, up to 200)</span><textarea value={words} onChange={e => setWords(e.target.value)} rows={5} /></label>
        <button type="submit" className="small">Save rules</button>
        <Status state={state} />
      </form>
    </Section>
    <Section title="Moderators" intro="Moderators can delete messages, time out and ban chatters, and change these rules. They can't act on you, each other or staff.">
      <ul className="list">{view.moderators.length === 0 ? <li className="muted">No moderators yet.</li> : view.moderators.map(m => <li key={m.username} className="row between"><UserChip user={m} />{view.role === "owner" && m.username && <button type="button" className="small quiet" onClick={() => dismiss(m.username!)}>Remove</button>}</li>)}</ul>
      {view.role === "owner" && <form onSubmit={appoint} className="row"><label className="field"><span>Add a moderator by username</span><input name="username" required /></label><button type="submit" className="small">Add</button></form>}
      <Status state={modState} />
    </Section>
    <Section title="Timeouts and bans" intro="A ban stops someone chatting and watching here while signed in.">
      <ul className="list">{view.restrictions.length === 0 ? <li className="muted">Nobody is timed out or banned.</li> : view.restrictions.map(r => <li key={`${r.user.username}-${r.kind}`} className="row between">
        <span><UserChip user={r.user} /> {r.kind === "ban" ? "Banned" : `Timed out until ${when(r.until!)}`}</span>
        {r.user.username && <button type="button" className="small quiet" onClick={() => lift(r.user.username!, r.kind)}>Lift</button>}
      </li>)}</ul>
    </Section>
    <Section title="Moderation log" intro="The latest 50 actions in your chat.">
      <ul className="list">{view.log.length === 0 ? <li className="muted">Nothing yet.</li> : view.log.map((l, i) => <li key={i}>
        <span className="muted">{when(l.created_at)}</span> {l.actor ? `@${l.actor}` : "Someone"} ({l.actor_role}) {label(l.action)}{l.target && ` @${l.target}`}: {l.reason}
      </li>)}</ul>
    </Section>
  </>;
}
