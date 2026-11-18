import assert from "node:assert/strict";
import test from "node:test";
import { duplicate, freeId, fresh, move, push, redo, settle, start, undo, widthAt } from "./board-builder.ts";

test("undo and redo walk the edits, and a new edit drops the redo steps", () => {
  let h = push(push(start("a"), "b"), "c");
  h = undo(h);
  assert.equal(h.present, "b");
  assert.equal(redo(h).present, "c");
  h = undo(h);
  assert.equal(h.present, "a");
  assert.equal(undo(h), h, "nothing left to undo");
  h = push(h, "d");
  assert.deepEqual([h.present, h.future], ["d", []]);
  assert.equal(redo(h), h, "nothing to redo");
});

test("a run of the same edit is one undo step", () => {
  let h = start("");
  for (const text of ["H", "Hy", "Hype"]) h = push(h, text, "boom:label");
  h = push(h, "Hype!", "boom:cost");
  assert.equal(undo(h).present, "Hype");
  assert.equal(undo(undo(h)).present, "");
  // After an undo or a save, the same field starts a new step.
  assert.equal(undo(push(undo(h), "x", "boom:label")).present, "Hype");
  assert.equal(undo(push(settle(h, "saved"), "y", "boom:cost")).present, "saved");
});

test("history keeps the last 100 steps", () => {
  let h = start(0);
  for (let i = 1; i <= 150; i++) h = push(h, i);
  assert.equal(h.past.length, 100);
});

test("move puts a control in another's place", () => {
  assert.deepEqual(move(["a", "b", "c", "d"], 0, 2), ["b", "c", "a", "d"]);
  assert.deepEqual(move(["a", "b", "c", "d"], 3, 0), ["d", "a", "b", "c"]);
  const list = ["a", "b"];
  assert.equal(move(list, 0, 0), list);
  assert.equal(move(list, 0, 2), list);
  assert.equal(move(list, -1, 0), list);
});

test("dragging an edge snaps to 1 to 4 columns", () => {
  // A 400 px grid with 8 px gaps: columns are 94 px.
  assert.equal(widthAt(0, 90, 400), 1);
  assert.equal(widthAt(0, 190, 400), 2);
  assert.equal(widthAt(0, 400, 400), 4);
  assert.equal(widthAt(0, -50, 400), 1);
  assert.equal(widthAt(0, 900, 400), 4);
  assert.equal(widthAt(102, 300, 400), 2, "measured from the control's own left edge");
});

test("new and duplicated controls get their own valid IDs", () => {
  const taken = new Set(["button-1", "button-1-2", "boom"]);
  assert.equal(fresh("button", taken).id, "button-2");
  assert.equal(fresh("goal", taken).target, 100);
  assert.equal(fresh("label", taken).width, 4);
  const boom = { ...fresh("button", new Set()), id: "boom", label: "Boom", cost: 50, color: "ember" };
  const list = duplicate([boom, fresh("label", taken)], 0, taken);
  assert.deepEqual(list.map(c => c.id), ["boom", "boom-2", "label-1"]);
  assert.deepEqual({ ...list[1], id: "boom" }, boom, "everything but the ID is copied");
  assert.equal(freeId("a".repeat(24), new Set(["a".repeat(20)])).length, 22);
  const full = Array.from({ length: 24 }, (_, i) => ({ ...boom, id: `b${i}` }));
  assert.equal(duplicate(full, 0, taken), full, "a full screen stays as it is");
});
