"use client";
import Link from "next/link";
import { useCallback, useState } from "react";
import { send, useLoad } from "../lib/client-api";
import { Section } from "./Form";

type Sub = { username: string; display_name: string; tier: number; months: number; paid_through: string; card: boolean; auto_renew: boolean };

/** /wallet: active subscriptions; gifted or Valor months link to "Keep your subscription". */
export function MySubscriptions() {
  const [subs, setSubs] = useState<Sub[] | null>(null);
  const load = useCallback(async () => {
    const r = await send<{ subscriptions: Sub[] }>("GET", "/api/me/subscriptions");
    if (r.ok) setSubs(r.data.subscriptions);
  }, []);
  useLoad(load);
  if (!subs?.length) return null;
  const date = (iso: string) => new Date(iso).toLocaleDateString(undefined, { month: "long", day: "numeric" });
  return <Section title="Your subscriptions" intro="Gifted and Valor months don't renew. Keep one by card from the channel; nothing is charged until it ends.">
    <ul className="list">{subs.map(s => <li key={s.username} className="row between wrap">
      <span><Link href={`/${s.username}`}><strong>{s.display_name}</strong></Link> · Tier {s.tier} · {s.months} {s.months === 1 ? "month" : "months"} · {s.auto_renew ? `renews ${date(s.paid_through)}` : `ends ${date(s.paid_through)}`}</span>
      {!s.card && <Link href={`/${s.username}`} className="button small">Keep your subscription</Link>}
    </li>)}</ul>
  </Section>;
}
