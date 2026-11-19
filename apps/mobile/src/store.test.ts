import assert from "node:assert/strict";
import test from "node:test";
import { SAVED, claimMessage, claimRequest, dollars, mergePacks, packValor, shouldFinish, showsWebLink } from "./store.ts";

const packs = [
  { product: "tv.sver.app.valor.500", valor: 500 },
  { product: "tv.sver.app.valor.100", valor: 100 },
  { product: "tv.sver.app.valor.1000", valor: 1000 },
];

test("packs show only what the store returned, smallest first, at the store's price", () => {
  const merged = mergePacks(packs, [
    { id: "tv.sver.app.valor.1000", displayPrice: "$9.99" },
    { id: "tv.sver.app.valor.100", displayPrice: "$0.99" },
    { id: "tv.sver.app.valor.other", displayPrice: "$1.00" },
  ]);
  assert.deepEqual(merged, [
    { product: "tv.sver.app.valor.100", valor: 100, price: "$0.99" },
    { product: "tv.sver.app.valor.1000", valor: 1000, price: "$9.99" },
  ]);
  assert.deepEqual(mergePacks(packs, []), []);
});

test("a purchase is finished only after credited or void", () => {
  assert.equal(shouldFinish({ ok: true, data: { status: "credited", valor: 100, mine: true } }), true);
  assert.equal(shouldFinish({ ok: true, data: { status: "void" } }), true);
  assert.equal(shouldFinish({ ok: true, data: { status: "pending" } }), false);
  for (const status of [0, 403, 409, 429, 500, 503]) {
    assert.equal(shouldFinish({ ok: false, status, error: "x" }), false);
  }
});

test("claims carry the transaction id on iOS and only the token on Android", () => {
  const purchase = { transactionId: "2000000123", environmentIOS: "Sandbox", purchaseToken: "jws-or-token", productId: "tv.sver.app.valor.100" };
  assert.deepEqual(claimRequest("ios", purchase), { path: "/api/me/wallet/apple", body: { transaction_id: "2000000123", environment: "Sandbox" } });
  assert.deepEqual(claimRequest("android", purchase), { path: "/api/me/wallet/google", body: { token: "jws-or-token" } });
  assert.equal(claimRequest("ios", { purchaseToken: "x" }), null);
  assert.equal(claimRequest("android", { transactionId: "1", purchaseToken: null }), null);
});

test("messages after a claim", () => {
  assert.equal(claimMessage({ ok: true, data: { status: "credited", valor: 600, mine: true } }, 500), "500 Valor added. Thank you!");
  assert.equal(claimMessage({ ok: true, data: { status: "credited", valor: 600, mine: true } }, 10000), "10,000 Valor added. Thank you!");
  assert.equal(claimMessage({ ok: true, data: { status: "credited", valor: 0, mine: false } }, 500), "This purchase was added to the account that started it.");
  assert.equal(claimMessage({ ok: true, data: { status: "void" } }, 500), null);
  assert.equal(claimMessage({ ok: false, status: 503, error: "Down" }, 500), SAVED);
  assert.equal(claimMessage({ ok: false, status: 0, error: "Offline" }, 500), SAVED);
  assert.equal(claimMessage({ ok: false, status: 403, error: "Test purchases aren't accepted on this account." }, 500), "Test purchases aren't accepted on this account.");
});

test("pack Valor comes from the product id", () => {
  assert.equal(packValor("tv.sver.app.valor.2000"), 2000);
  assert.equal(packValor("tv.sver.app.other"), null);
});

test("the sver.tv link shows only in the US storefront, and on Google only with the flag and the program", () => {
  assert.equal(showsWebLink("ios", "USA", false, false), true);
  assert.equal(showsWebLink("ios", "US", true, true), false);
  assert.equal(showsWebLink("ios", "GBR", true, true), false);
  assert.equal(showsWebLink("ios", null, true, true), false);
  assert.equal(showsWebLink("android", "US", true, true), true);
  assert.equal(showsWebLink("android", "US", false, true), false);
  assert.equal(showsWebLink("android", "US", true, false), false);
  assert.equal(showsWebLink("android", "USA", true, true), false);
  assert.equal(showsWebLink("android", "DE", true, true), false);
});

test("dollars", () => {
  assert.equal(dollars(5000), "$50");
  assert.equal(dollars(1234), "$12.34");
  assert.equal(dollars(0), "$0");
});
