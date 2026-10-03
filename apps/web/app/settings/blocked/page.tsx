"use client";
import Link from "next/link";
import { useCallback, useState } from "react";
import { Section } from "../../../components/Form";
import { send, useLoad } from "../../../lib/client-api";

export default function Blocked() {
  const [items, setItems] = useState<{ username: string; blocked_at: string }[] | null>(null);
  const [error, setError] = useState("");
  const load = useCallback(async () => {
    const result = await send<{ items: { username: string; blocked_at: string }[] }>("GET", "/api/me/blocks");
    if (result.ok) setItems(result.data.items); else setError(result.error);
  }, []);
  useLoad(load);
  async function unblock(name: string) {
    const result = await send("DELETE", `/api/blocks/${encodeURIComponent(name)}`);
    if (result.ok) load(); else setError(result.error);
  }
  return <><h1>Blocked users</h1>
    <Section title="People you've blocked" intro="Blocked users can still see your public channel, but you can't interact with each other. They aren't told.">
      {error && <p role="alert" className="form-message error">{error}</p>}
      {!items ? <p className="loading">Loading…</p> : items.length === 0 ? <p className="muted">You haven&apos;t blocked anyone.</p> :
        <ul className="list">{items.map(i => <li key={i.username} className="row between"><Link href={`/${i.username}`}>@{i.username}</Link><button type="button" className="small quiet" onClick={() => unblock(i.username)}>Unblock</button></li>)}</ul>}
    </Section></>;
}
