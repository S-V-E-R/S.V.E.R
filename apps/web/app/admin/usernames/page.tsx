"use client";
import Link from "next/link";
import { useCallback, useState } from "react";
import { Section } from "../../../components/Form";
import { send, useLoad } from "../../../lib/client-api";

type Claim = { id: string; username: string; status: string; created_at: string; requester: string | null; reason: string | null };

/** Staff → Username claims: release or reject names whose inactive holder didn't sign in within 30 days. */
export default function AdminUsernames() {
  const [claims, setClaims] = useState<Claim[] | null>(null);
  const [message, setMessage] = useState("");
  const load = useCallback(async () => {
    const r = await send<{ claims: Claim[] }>("GET", "/api/admin/username-claims");
    if (r.ok) setClaims(r.data.claims); else setMessage(r.error);
  }, []);
  useLoad(load);
  async function decide(claim: Claim, action: "release" | "reject") {
    const reason = window.prompt(action === "release" ? `Release @${claim.username} to @${claim.requester}? Reason:` : "Why is it not released? The requester sees this.")?.trim();
    if (!reason) return;
    const r = await send("POST", `/api/admin/username-claims/${encodeURIComponent(claim.id)}`, { action, reason });
    setMessage(r.ok ? `${action === "release" ? "Released" : "Rejected"}: @${claim.username}.` : r.error);
    await load();
  }
  if (!claims) return message ? <p role="alert" className="form-message error">{message}</p> : <p className="loading">Loading…</p>;
  const ready = claims.filter(c => c.status === "READY");
  const rest = claims.filter(c => c.status !== "READY");
  const row = (c: Claim) => <li key={c.id}>
    <p><strong>@{c.username}</strong> asked by {c.requester ? <Link href={`/${c.requester}`}>@{c.requester}</Link> : "a deleted account"} · {new Date(c.created_at).toLocaleDateString()} · {c.status}</p>
    {c.reason && <p className="muted small">{c.reason}</p>}
    {c.status === "READY" && <div className="row"><button type="button" className="small" onClick={() => decide(c, "release")}>Release</button><button type="button" className="small quiet" onClick={() => decide(c, "reject")}>Reject</button></div>}
  </li>;
  return <><h1>Username claims</h1>
    <p>A claim reaches this list after the holder, inactive for 12 months, was emailed and didn&apos;t sign in within 30 days. Releasing renames the holder to a generated name and gives the requester this one; it re-checks inactivity first. Every decision is audited.</p>
    <Section title={`Ready · ${ready.length}`}>{ready.length === 0 ? <p className="muted">Nothing to decide.</p> : <ul className="list">{ready.map(row)}</ul>}</Section>
    <Section title="Other claims">{rest.length === 0 ? <p className="muted">None.</p> : <ul className="list">{rest.map(row)}</ul>}</Section>
    {message && <p role="status" className="form-message">{message}</p>}
  </>;
}
