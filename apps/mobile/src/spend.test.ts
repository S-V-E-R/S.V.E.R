import assert from "node:assert/strict";
import test from "node:test";
import { Attempt, aboutValor, parseTribute } from "./spend.ts";

const counter = () => {
  let n = 0;
  return () => `id-${(n += 1)}`;
};

test("an unanswered try keeps its id", () => {
  for (const status of [0, 500, 503, 429]) {
    const attempt = new Attempt(counter());
    assert.equal(attempt.take("tier:1"), "id-1");
    assert.equal(attempt.reused, false);
    attempt.settle({ ok: false, status });
    assert.equal(attempt.take("tier:1"), "id-1");
    assert.equal(attempt.reused, true);
  }
});

test("an answer or a change starts a new id", () => {
  const attempt = new Attempt(counter());
  attempt.take("tier:1");
  attempt.settle({ ok: true });
  assert.equal(attempt.take("tier:1"), "id-2");
  attempt.settle({ ok: false, status: 400 });
  assert.equal(attempt.take("tier:1"), "id-3");
  attempt.settle({ ok: false, status: 0 });
  assert.equal(attempt.take("tier:2"), "id-4");
  assert.equal(attempt.reused, false);
});

test("a tribute is a whole number of at least 10 Valor", () => {
  assert.equal(parseTribute("10"), 10);
  assert.equal(parseTribute("25.9"), 25);
  assert.equal(parseTribute("9.99"), null);
  assert.equal(parseTribute(""), null);
  assert.equal(parseTribute("abc"), null);
  assert.equal(parseTribute("-50"), null);
});

test("Valor refusals offer Get Valor", () => {
  assert.equal(aboutValor("You don't have enough Valor."), true);
  assert.equal(aboutValor("Your Valor is locked until a payment problem is settled."), true);
  assert.equal(aboutValor("This channel can't receive tributes yet."), false);
});
