"use client";
import Link from "next/link";
import { useCallback, useState } from "react";
import { Section, Status, type SaveState } from "../../../components/Form";
import { send, useLoad } from "../../../lib/client-api";
import { founderNumber, tierName } from "../../../lib/supporters";
import type { SupporterTier } from "../../../components/Supporter";

type Subscriber = { username: string; tier: SupporterTier; pending_tier: number | null; paid_through: string; auto_renew: boolean; months: number; since: string; founder: number | null };
type Founder = { number: number; username: string; hidden: boolean; claimed_at: string };
type Admin = { subscribers: Subscriber[]; founders: Founder[]; paid_cents: number; loaded: number };
const date = (iso: string) => new Date(iso).toLocaleDateString();

function Action({ path, label, ask, reload }: { path: string; label: string; ask: string; reload: () => void }) {
  const [state, setState] = useState<SaveState>({});
  async function run() {
    const note = window.prompt(ask);
    if (!note) return;
    const r = await send("POST", path, { note });
    setState(r.ok ? { saved: "Done." } : r);
    if (r.ok) reload();
  }
  return <><button type="button" className="small quiet danger-text" onClick={run}>{label}</button><Status state={state} /></>;
}

/** Platform supporters and Founders (docs/SUPPORTERS.md "Admin"). Refunds and revokes are audited. */
export default function SupportersAdmin() {
  const [admin, setAdmin] = useState<Admin | null>(null);
  const [error, setError] = useState("");
  const load = useCallback(async () => {
    const r = await send<Admin>("GET", "/api/admin/supporters");
    if (r.ok) setAdmin({ ...r.data, loaded: Date.now() }); else setError(r.error);
  }, []);
  useLoad(load);
  if (!admin) return <p className="loading">{error || "Loading…"}</p>;
  const now = admin.loaded;
  return <><h1>Supporters</h1>
    <p>{admin.subscribers.filter(s => Date.parse(s.paid_through) > now).length} active · {admin.founders.length} of 500 Founders · ${(admin.paid_cents / 100).toLocaleString("en-US", { minimumFractionDigits: 2 })} paid in total. New subscriptions pause with the &ldquo;Supporter sales&rdquo; <Link href="/admin/switches">switch</Link>; existing ones keep running.</p>
    <Section title="Subscribers" intro="A refund returns the latest paid month in full, ends the plan and frees a Founder number that payment claimed.">
      {admin.subscribers.length === 0 ? <p className="muted">No supporters yet.</p> : <ul className="list">{admin.subscribers.map(s => <li key={s.username} className="row between wrap">
        <span><Link href={`/admin/users/${s.username}`}>@{s.username}</Link> · {tierName[s.tier]}{s.pending_tier ? " (moving down at renewal)" : ""} · {s.months} {s.months === 1 ? "month" : "months"} · {Date.parse(s.paid_through) > now ? `${s.auto_renew ? "renews" : "ends"} ${date(s.paid_through)}` : `ended ${date(s.paid_through)}`}{s.founder ? ` · Founder ${founderNumber(s.founder)}` : ""}</span>
        <span className="row wrap"><Action path={`/api/admin/supporters/${encodeURIComponent(s.username)}/refund`} label="Refund latest month" ask={`Why refund @${s.username}'s latest month in full? (audited)`} reload={load} /></span>
      </li>)}</ul>}
    </Section>
    <Section title="Founders" intro="Staff can't grant Founder marks. Revoking frees the number for the next new Greycloak.">
      {admin.founders.length === 0 ? <p className="muted">No Founders yet.</p> : <ul className="list">{admin.founders.map(f => <li key={f.number} className="row between wrap">
        <span>{founderNumber(f.number)} · <Link href={`/admin/users/${f.username}`}>@{f.username}</Link> · claimed {date(f.claimed_at)}{f.hidden ? " · name hidden" : ""}</span>
        <span className="row wrap"><Action path={`/api/admin/supporters/founders/${f.number}/revoke`} label="Revoke mark" ask={`Why revoke Founder ${founderNumber(f.number)}? (audited)`} reload={load} /></span>
      </li>)}</ul>}
    </Section>
  </>;
}
