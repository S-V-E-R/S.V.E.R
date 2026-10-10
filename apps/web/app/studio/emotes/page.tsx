"use client";
import { FormEvent, useCallback, useState } from "react";
import { Section, Status, type SaveState } from "../../../components/Form";
import { EmoteImage, providerNames, type ChannelEmote, type OutsideProvider } from "../../../components/Emote";
import { send, useLoad } from "../../../lib/client-api";

type Item = ChannelEmote & { status: "VISIBLE" | "REMOVED" | "UNAVAILABLE"; signature?: boolean; reviewed?: boolean };
export default function EmotesStudio() {
  const [items, setItems] = useState<Item[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [busy, setBusy] = useState(false);
  const [state, setState] = useState<SaveState>({});
  const [tier, setTier] = useState("");
  const load = useCallback(async () => {
    const r = await send<{ items: Item[] }>("GET", "/api/me/emotes");
    if (r.ok) { setItems(r.data.items); setLoaded(true); } else setState(r);
  }, []);
  useLoad(load);
  async function upload(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    setBusy(true);
    const r = await send("POST", "/api/me/emotes", new FormData(form));
    setBusy(false); setState(r.ok ? { saved: "Emote added." } : r);
    if (r.ok) { form.reset(); await load(); }
  }
  async function remove(item: Item) {
    if (!window.confirm(`Delete ${item.code}? Future chat rendering will show its code as text.`)) return;
    setBusy(true);
    const r = await send("DELETE", `/api/me/emotes/${encodeURIComponent(item.id)}`);
    setBusy(false); setState(r.ok ? { saved: "Emote deleted." } : r);
    if (r.ok) await load();
  }
  async function signature(item: Item) {
    const r = await send("PUT", `/api/me/emotes/${encodeURIComponent(item.id)}/signature`, { on: !item.signature });
    if (r.ok) await load(); else setState(r);
  }
  const open = items.filter(i => !i.tier).length;
  const full = tier ? items.filter(i => i.tier === Number(tier)).length >= 5 : open >= 10;
  return <><h1>Channel emotes</h1><p>Open emotes are for everyone in your chat; subscriber emotes are for subscribers of that tier or higher. Codes are case-sensitive and must be typed as a whole word, separated by spaces.</p>
    <Section title={`Emotes · ${open}/10 open · ${[1, 2, 3].map(t => `Tier ${t} ${items.filter(i => i.tier === t).length}/5`).join(" · ")}`}>
      {!loaded ? <p>Loading…</p> : items.length === 0 ? <p className="muted">No emotes yet.</p> : <ul className="list">{items.map(item => <li className="row between" key={item.id}><span className="row">{item.status === "VISIBLE" && <EmoteImage emote={item} />}<strong>{item.code}</strong>{item.tier && <span className="badge sub-badge">Tier {item.tier}</span>}{item.status !== "VISIBLE" && <span className="muted">{item.status === "REMOVED" ? "Removed by staff" : "Unavailable"}</span>}</span><span className="row">{!item.tier && item.status === "VISIBLE" && <button type="button" className="quiet small" disabled={busy} onClick={() => void signature(item)}>{item.signature ? (item.reviewed ? "Signature · works everywhere" : "Signature · awaiting staff review") : "Make signature"}</button>}<button className="quiet small danger-text" disabled={busy} onClick={() => remove(item)}>Delete<span className="sr-only"> {item.code}</span></button></span></li>)}</ul>}
      <p className="muted small">Your signature emote (one open emote) works in every chat, written as yourname/Code, once staff approve it. Channels can turn signature emotes off in their chat.</p>
    </Section>
    <Section title="Upload emote"><form onSubmit={upload}>
      <p className="muted">Square PNG or WebP, at least 112 × 112 pixels, up to 1 MB. Animated images use their first frame. Emotes publish immediately.</p>
      <label className="field narrow"><span>Code</span><input name="code" required minLength={3} maxLength={20} pattern="[A-Za-z0-9]{3,20}" autoComplete="off" /><small>3–20 ASCII letters and digits.</small></label>
      <label className="field narrow"><span>Who can use it</span><select name="tier" value={tier} onChange={e => setTier(e.target.value)}><option value="">Everyone (open)</option><option value="1">Tier 1 subscribers</option><option value="2">Tier 2 subscribers</option><option value="3">Tier 3 subscribers</option></select></label>
      <label className="field"><span>Image</span><input type="file" name="file" required accept="image/png,image/webp" /></label>
      <button disabled={busy || !loaded || full}>Upload emote</button>
    </form></Section><Status state={state} /><OutsideEmotes /></>;
}

type Outside = { available: OutsideProvider[]; providers: OutsideProvider[]; global: boolean; twitch_linked: boolean; synced_at: string | null;
  emotes: (ChannelEmote & { provider: OutsideProvider; global: boolean; hidden: "streamer" | "report" | null })[] };
/** 7TV, BTTV and FFZ emotes in chat (docs/DEVELOPER_PLATFORM.md §7). */
function OutsideEmotes() {
  const [data, setData] = useState<Outside | null>(null);
  const [state, setState] = useState<SaveState>({});
  const load = useCallback(async () => {
    const r = await send<Outside>("GET", "/api/me/outside-emotes");
    if (r.ok) setData(r.data); else setState(r);
  }, []);
  useLoad(load);
  async function save(providers: OutsideProvider[], global: boolean) {
    const r = await send<Outside>("PUT", "/api/me/outside-emotes", { providers, global });
    if (r.ok) { setData(r.data); setState({ saved: "Saved. New emotes appear within a few minutes." }); } else setState(r);
  }
  async function hide(emote: Outside["emotes"][number], hidden: boolean) {
    const r = await send<Outside>("PUT", `/api/me/outside-emotes/${emote.provider}/${encodeURIComponent(emote.id)}`, { hidden });
    if (r.ok) setData(r.data); else setState(r);
  }
  if (!data) return null;
  if (data.available.length === 0) return <Section title="7TV, BTTV and FFZ emotes"><p className="muted">Coming soon: show your 7TV, BTTV and FFZ emotes in S.V.E.R chat.</p></Section>;
  const reported = data.emotes.filter(e => e.hidden === "report");
  return <Section title="7TV, BTTV and FFZ emotes">
    <p>Show the emotes you use on Twitch. S.V.E.R copies their images, so viewers never connect to those services. Your own emotes win when names clash, and your banned words apply.</p>
    {!data.twitch_linked && <p className="muted">Link your Twitch account in Settings first; your emotes are found through it.</p>}
    <fieldset><legend>Services</legend>{data.available.map(p => <label key={p} className="checkbox"><input type="checkbox" checked={data.providers.includes(p)} onChange={e => void save(e.target.checked ? [...data.providers, p] : data.providers.filter(x => x !== p), data.global)} />{providerNames[p]}</label>)}</fieldset>
    <label className="checkbox"><input type="checkbox" checked={data.global} onChange={e => void save(data.providers, e.target.checked)} />Also show their global emotes</label>
    {reported.length > 0 && <p role="status"><strong>{reported.length} reported by viewers.</strong> They stay hidden until you show them again.</p>}
    {data.emotes.length > 0 && <ul className="list">{data.emotes.map(e => <li className="row between" key={`${e.provider}:${e.id}`}>
      <span className="row"><EmoteImage emote={e} /><strong>{e.code}</strong><span className="muted small">{providerNames[e.provider]}{e.global && " · global"}</span>{e.hidden && <span className="badge">{e.hidden === "report" ? "Reported" : "Hidden"}</span>}</span>
      <button type="button" className="quiet small" onClick={() => void hide(e, !e.hidden)}>{e.hidden ? "Show" : "Hide"}<span className="sr-only"> {e.code}</span></button>
    </li>)}</ul>}
    <Status state={state} />
  </Section>;
}
