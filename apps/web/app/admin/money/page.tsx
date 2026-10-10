"use client";
import { FormEvent, useCallback, useState } from "react";
import { send, useLoad } from "../../../lib/client-api";

type Row = Record<string, string | number | boolean | null>;
type Money = {
  payments: Row[]; invoices: Row[]; tributes: Row[]; payouts: Row[]; reversals: Row[]; negative: Row[]; accounts: Row[];
  subscriptions: { active: number; by_card: number; tier1: number; tier2: number; tier3: number }; max_adjust: number;
};
const usd = (cents: unknown) => typeof cents === "number" ? `$${(cents / 100).toFixed(2)}` : "";
const when = (at: unknown) => typeof at === "string" ? new Date(at).toLocaleString() : "";

function Table({ caption, rows, columns }: { caption: string; rows: Row[]; columns: [string, string, (r: Row) => React.ReactNode][] }) {
  return <section><h2>{caption}</h2>
    {rows.length === 0 ? <p className="muted">None yet.</p> : <div className="table-scroll"><table className="table">
      <thead><tr>{columns.map(([key, label]) => <th key={key} scope="col">{label}</th>)}</tr></thead>
      <tbody>{rows.map((r, i) => <tr key={i}>{columns.map(([key, , cell]) => <td key={key}>{cell(r)}</td>)}</tr>)}</tbody>
    </table></div>}
  </section>;
}

/** Money (docs/ADMIN.md "Money"): read-only views; refunds through Stripe; Valor adjustments with a reason. */
export default function MoneyPage() {
  const [data, setData] = useState<Money | null>(null);
  const [message, setMessage] = useState("");
  const load = useCallback(async () => {
    const r = await send<Money>("GET", "/api/admin/money");
    if (r.ok) setData(r.data); else setMessage(r.error);
  }, []);
  useLoad(load);
  async function refund(event: FormEvent<HTMLFormElement>, pi: string) {
    event.preventDefault();
    if (!window.confirm(`Refund ${pi} in full through Stripe?`)) return;
    const r = await send<Money>("POST", "/api/admin/money/refund", { payment_intent: pi, note: new FormData(event.currentTarget).get("note") });
    if (r.ok) { setData(r.data); setMessage("Refund sent to Stripe. The ledger reverses when Stripe confirms it."); } else setMessage(r.error);
  }
  async function adjust(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    const f = new FormData(form);
    const r = await send<Money>("POST", "/api/admin/money/valor", { username: f.get("username"), valor: Number(f.get("valor")), note: f.get("note") });
    if (r.ok) { setData(r.data); form.reset(); setMessage("Valor adjusted."); } else setMessage(r.error);
  }
  const refundCell = (r: Row) => (r.status === "paid" || r.tier !== undefined) && typeof r.payment_intent === "string"
    ? <form className="row" onSubmit={e => void refund(e, r.payment_intent as string)}>
      <label className="sr-only" htmlFor={`note-${r.payment_intent}`}>Refund note</label><input id={`note-${r.payment_intent}`} name="note" required maxLength={500} placeholder="Note" />
      <button type="submit" className="small quiet danger-text">Refund</button></form>
    : null;
  return <section className="panel section"><h1>Money</h1>
    <p className="intro">Stripe holds card and bank details; none show here. Every action needs a note and is audited.</p>
    {message && <p role="status" className="form-message">{message}</p>}
    {!data ? <p className="loading">Loading…</p> : <>
      <p><strong>{data.subscriptions.active}</strong> active subscriptions ({data.subscriptions.by_card} by card) · Tier 1 {data.subscriptions.tier1} · Tier 2 {data.subscriptions.tier2} · Tier 3 {data.subscriptions.tier3}</p>
      <Table caption="Payments" rows={data.payments} columns={[["at", "When", r => when(r.created_at)], ["user", "Buyer", r => r.user], ["kind", "Kind", r => r.kind], ["cents", "Amount", r => usd(r.cents)], ["status", "Status", r => r.status], ["refund", "Refund", refundCell]]} />
      <Table caption="Subscription invoices" rows={data.invoices} columns={[["at", "When", r => when(r.created_at)], ["user", "Subscriber", r => r.user], ["channel", "Channel", r => r.channel], ["tier", "Tier", r => r.tier], ["cents", "Amount", r => usd(r.cents)], ["refund", "Refund", refundCell]]} />
      <Table caption="Tributes" rows={data.tributes} columns={[["at", "When", r => when(r.created_at)], ["from", "From", r => r.from], ["channel", "Channel", r => r.channel], ["valor", "Valor", r => r.valor]]} />
      <Table caption="Payouts and Early Pay" rows={data.payouts} columns={[["at", "When", r => when(r.created_at)], ["user", "Creator", r => r.user], ["kind", "Kind", r => r.kind], ["cents", "Amount", r => usd(r.cents)], ["fee", "Fee", r => usd(r.fee_cents)], ["status", "Status", r => <>{r.status}{r.error && <span className="muted small"> · {r.error}</span>}</>]]} />
      <Table caption="Refunds and chargebacks" rows={data.reversals} columns={[["at", "When", r => when(r.created_at)], ["kind", "Kind", r => r.kind], ["pi", "Payment", r => <code>{r.payment_intent}</code>], ["cents", "Amount", r => usd(r.cents)]]} />
      <Table caption="Negative balances" rows={data.negative} columns={[["user", "Account", r => r.user], ["unit", "Unit", r => r.unit === "usd" ? "Earnings" : "Valor"], ["balance", "Balance", r => r.unit === "usd" ? usd(Number(r.balance) / 10) : r.balance]]} />
      <Table caption="Payout accounts and tax forms" rows={data.accounts} columns={[["user", "Creator", r => r.user], ["tax", "Tax information", r => r.tax_complete ? "Complete" : "Not complete"], ["payouts", "Payouts", r => r.payouts_enabled ? "Enabled" : "Not yet"], ["guardian", "Guardian", r => r.guardian ? (r.payouts_enabled ? "Approved" : "Waiting for guardian") : ""]]} />
      <h2>Adjust Valor</h2>
      <form className="stack" onSubmit={adjust}>
        <label className="field narrow"><span>Username</span><input name="username" required maxLength={30} /></label>
        <label className="field narrow"><span>Valor (negative to remove, up to {data.max_adjust.toLocaleString()})</span><input name="valor" type="number" required min={-data.max_adjust} max={data.max_adjust} step={1} /></label>
        <label className="field"><span>Reason (required)</span><input name="note" required maxLength={500} /></label>
        <button type="submit">Post adjustment</button>
      </form>
    </>}
  </section>;
}
