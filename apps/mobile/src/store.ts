import type { Result } from "./api";

/** One Valor pack both stores sell (`store::PACKS` in the API). */
export type StorePack = { product: string; valor: number };

/** `GET /api/me/wallet`. `store` lists the packs; each flag is true when that store is set up on the server. */
export type Wallet = {
  valor: number;
  locked: boolean;
  minor: boolean;
  guardian_confirmed: boolean;
  month_cents: number | null;
  cap_cents: number | null;
  store?: { apple: boolean; google: boolean; google_us_link?: boolean; packs: StorePack[] };
};

/** What the claim endpoints answer. `valor` is the caller's balance; `mine` is false when another account started the purchase. */
export type Claim = { status: "credited"; valor: number; mine: boolean } | { status: "pending" } | { status: "void" };

export type Os = "ios" | "android";

/** Packs the server and the store both have, smallest first, priced by the store (never a hard-coded price). */
export function mergePacks(packs: StorePack[], products: { id: string; displayPrice: string }[]): (StorePack & { price: string })[] {
  const prices = new Map(products.map(p => [p.id, p.displayPrice]));
  return packs.flatMap(pack => {
    const price = prices.get(pack.product);
    return price ? [{ ...pack, price }] : [];
  }).sort((a, b) => a.valor - b.valor);
}

/** The claim request for a store purchase, or null when the purchase lacks the id the server reads. */
export function claimRequest(os: Os, purchase: { transactionId?: string | null; environmentIOS?: string | null; purchaseToken?: string | null }): { path: string; body: Record<string, string> } | null {
  if (os === "ios") {
    return purchase.transactionId ? { path: "/api/me/wallet/apple", body: { transaction_id: purchase.transactionId, environment: purchase.environmentIOS ?? "" } } : null;
  }
  // The server reads the product from Google itself, so the app sends only the token.
  return purchase.purchaseToken ? { path: "/api/me/wallet/google", body: { token: purchase.purchaseToken } } : null;
}

/**
 * Finish a store transaction only after the server credited it or found nothing to credit. Any other
 * answer leaves it unfinished so the store hands it back on the next launch.
 */
export function shouldFinish(result: Result<Claim>): boolean {
  return result.ok && (result.data.status === "credited" || result.data.status === "void");
}

/** Valor in a pack, from its product id (`tv.sver.app.valor.500`; the API's PACKS test pins the suffix). */
export function packValor(product: string): number | null {
  const match = /\.valor\.(\d+)$/.exec(product);
  return match ? Number(match[1]) : null;
}

export const SAVED = "Your purchase is saved on this device and is added the next time the app opens.";

/** What to tell the buyer after a claim, or null for nothing. `valor` is the purchase's Valor when known. */
export function claimMessage(result: Result<Claim>, valor: number | null): string | null {
  if (!result.ok) {
    // 409, rate limits, 5xx and no connection are retried on the next launch; other refusals won't change.
    const retried = result.status === 0 || result.status === 409 || result.status === 429 || result.status >= 500;
    return retried ? SAVED : result.error;
  }
  if (result.data.status !== "credited") return null;
  if (!result.data.mine) return "This purchase was added to the account that started it.";
  return valor ? `${valor.toLocaleString("en-US")} Valor added. Thank you!` : "Valor added. Thank you!";
}

/**
 * The quiet US-only line to sver.tv's packs. Apple: the US storefront (alpha-3). Google: the US
 * (alpha-2), only while the server flag is on and Play says the external content links program is
 * available on this device.
 */
export function showsWebLink(os: Os, country: string | null, googleFlag: boolean, googleProgram: boolean): boolean {
  if (os === "ios") return country === "USA";
  return country === "US" && googleFlag && googleProgram;
}

/** "$50" or "$12.34". */
export function dollars(cents: number): string {
  return `$${cents % 100 === 0 ? cents / 100 : (cents / 100).toFixed(2)}`;
}
