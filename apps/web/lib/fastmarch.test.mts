import assert from "node:assert/strict";
import test from "node:test";
import { newGame, step, type Game } from "./fastmarch.ts";

const run = (g: Game, ticks: number) => { for (let i = 0; i < ticks; i++) step(g); return g; };

test("the same seed plays the same race", () => {
  const a = run(newGame(42), 300), b = run(newGame(42), 300);
  assert.deepEqual(a.riders, b.riders);
  assert.deepEqual(a.cells, b.cells);
});

test("a courier that runs into the wall is lost, and its trail stays", () => {
  const g = newGame(1);
  const startY = g.riders[0].y;
  run(g, startY + 1); // the player never turns and drives up into the wall
  assert.equal(g.riders[0].alive, false);
  assert.equal(g.riders[0].diedAt, startY + 1);
  assert.equal(g.cells[0 * g.w + g.riders[0].x], 2);
});

test("computer couriers find room instead of driving straight into the Static", () => {
  for (const seed of [1, 2, 3, 4, 5]) {
    const g = run(newGame(seed), 40);
    assert.ok(g.riders[1].alive && g.riders[2].alive, `seed ${seed}`);
  }
});

test("a reverse turn is ignored", () => {
  const g = newGame(3);
  step(g, 2); // down while heading up
  assert.equal(g.riders[0].dir, 0);
});
