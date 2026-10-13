"use client";
import Link from "next/link";
import { FormEvent, useCallback, useState } from "react";
import { Section, Status, type SaveState } from "../../../components/Form";
import { send, useLoad } from "../../../lib/client-api";
import "../../../styles/wall.css";
import { dollars, place, sizeLabel, stageName, statusLabel, type MyTile, type WallSize } from "../../../lib/wall";

type AdminTile = MyTile & { owner: string | null; payment_intent: string | null; created_at: string };
type Admin = { tiles: AdminTile[]; sold_cells: number; sales_cents: number; stage: number; prices: Record<WallSize, number[]>; stages: number[]; limits: Record<"table" | "hall", number>; sales_opened_at: string | null };
const sizes: WallSize[] = ["seat", "table", "hall"];

/** Prices and limits: anything before sales open, only stages not reached yet after. */
function Settings({ admin, reload }: { admin: Admin; reload: () => void }) {
  const [state, setState] = useState<SaveState>({});
  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const prices = Object.fromEntries(sizes.map(s => [s, admin.prices[s].map((_, i) => Math.round(Number(form.get(`${s}-${i}`)) * 100))]));
    const limits = { table: Number(form.get("limit-table")), hall: Number(form.get("limit-hall")) };
    const r = await send("PUT", "/api/admin/wall/settings", { prices, limits, note: form.get("note") });
    setState(r.ok ? { saved: "Saved." } : r);
    if (r.ok) reload();
  }
  async function open(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const r = await send("POST", "/api/admin/wall/open", { note: new FormData(event.currentTarget).get("note") });
    setState(r.ok ? { saved: "Sales are open." } : r);
    if (r.ok) reload();
  }
  const open_ = !!admin.sales_opened_at;
  return <Section title="Prices and limits" intro={open_ ? `Sales opened ${new Date(admin.sales_opened_at!).toLocaleString()}. Only prices of stages not reached yet can change; limits are fixed.` : "Sales are closed. Open them only after the Wall terms, tax and prices are settled (docs/WALL.md, Before sales open)."}>
    <form onSubmit={save} className="wall-form">
      <div className="table-scroll"><table className="table"><thead><tr><th scope="col">Size</th>{admin.prices.seat.map((_, i) => <th key={i} scope="col">{stageName(i)}{i > 0 && ` (from ${admin.stages[i - 1]} cells)`}</th>)}</tr></thead>
        <tbody>{sizes.map(s => <tr key={s}><th scope="row">{sizeLabel[s]}</th>{admin.prices[s].map((cents, i) => <td key={i}><input aria-label={`${sizeLabel[s]}, ${stageName(i)}, dollars`} name={`${s}-${i}`} type="number" min={1} step="0.01" defaultValue={cents / 100} disabled={open_ && i <= admin.stage} /></td>)}</tr>)}</tbody></table></div>
      <div className="row wrap"><label className="field narrow"><span>Tables at most</span><input name="limit-table" type="number" min={0} max={250} defaultValue={admin.limits.table} disabled={open_} /></label><label className="field narrow"><span>Halls at most</span><input name="limit-hall" type="number" min={0} max={250} defaultValue={admin.limits.hall} disabled={open_} /></label></div>
      <label className="field"><span>Note (required, audited)</span><input name="note" required maxLength={500} /></label>
      <button className="small">Save prices and limits</button>
    </form>
    {!open_ && <form onSubmit={open} className="wall-form"><label className="field"><span>Why sales can open now (required, audited)</span><input name="note" required maxLength={500} /></label><button className="small danger">Open sales</button></form>}
    <Status state={state} />
  </Section>;
}

function TileRow({ tile, reload }: { tile: AdminTile; reload: () => void }) {
  const [state, setState] = useState<SaveState>({});
  async function act(action: string, ask: string) {
    const note = window.prompt(ask);
    if (!note) return;
    const r = await send("POST", `/api/admin/wall/${tile.n}/${action}`, { note });
    setState(r.ok ? { saved: "Saved." } : r);
    if (r.ok) reload();
  }
  const paid = !!tile.paid_at && tile.status !== "refund_due" && tile.status !== "refunded";
  return <li className="row between wrap">
    <span><Link href={`/wall/${tile.n}`}>Tile {tile.n}</Link> · {tile.name ?? "—"} · {sizeLabel[tile.size]} at {place(tile)} · {tile.owner ? <Link href={`/admin/users/${tile.owner}`}>@{tile.owner}</Link> : "no owner"} · {dollars(tile.price_cents)}</span>
    <span className="row wrap"><span className="badge">{tile.status === "held" ? "Held, unpaid" : statusLabel[tile.status]}{tile.pending ? " · change waiting" : ""}</span>
      {tile.status === "live" && tile.link && <button type="button" className="small quiet" onClick={() => act("unlink", "Why remove the link? (audited)")}>Remove link</button>}
      {tile.status === "live" && <button type="button" className="small quiet danger-text" onClick={() => act("remove", "Why remove this tile under the Wall terms? (audited)")}>Remove tile</button>}
      {paid && <button type="button" className="small quiet" onClick={() => act("refund", "Why refund this tile in full? (audited)")}>Refund</button>}
      <Status state={state} /></span>
  </li>;
}

export default function WallAdmin() {
  const [admin, setAdmin] = useState<Admin | null>(null);
  const [error, setError] = useState("");
  const load = useCallback(async () => {
    const r = await send<Admin>("GET", "/api/admin/wall");
    if (r.ok) setAdmin(r.data); else setError(r.error);
  }, []);
  useLoad(load);
  if (!admin) return <p className="loading">{error || "Loading…"}</p>;
  return <><h1>The Wall</h1>
    <p>{admin.sold_cells} of 1,000 cells sold · {stageName(admin.stage)} · {dollars(admin.sales_cents)} in sales. Tiles and changes are reviewed in <Link href="/admin/media">Media review</Link>; sales pause with the &ldquo;Wall sales&rdquo; <Link href="/admin/switches">switch</Link>.</p>
    <Settings admin={admin} reload={load} />
    <Section title="Tiles">{admin.tiles.length === 0 ? <p className="muted">No tiles yet.</p> : <ul className="list">{admin.tiles.map(t => <TileRow key={t.n} tile={t} reload={load} />)}</ul>}</Section>
  </>;
}
