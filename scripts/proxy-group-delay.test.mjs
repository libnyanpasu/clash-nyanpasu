import assert from "node:assert/strict";
import test from "node:test";
import { getGroupSelectedDelay } from "../frontend/nyanpasu/src/components/proxies/group-delay.ts";

const node = (name, delays = []) => ({
  name,
  all: null,
  now: null,
  history: delays.map((delay) => ({ time: "", delay })),
});
const group = (name, now, all) => ({ name, now, all });
const snapshot = (groups, records = {}) => ({
  global: group("GLOBAL", groups[0]?.name, groups),
  groups,
  records,
});

test("uses the selected member latest measurement, including failure and no history", () => {
  const selected = node("selected", [30, 80]);
  const root = group("root", "selected", [node("other", [10]), selected]);
  const data = snapshot([root]);
  assert.equal(getGroupSelectedDelay(root, data), 80);
  selected.history.push({ time: "", delay: 0 });
  assert.equal(getGroupSelectedDelay(root, data), 0);
  selected.history = [];
  assert.equal(getGroupSelectedDelay(root, data), undefined);
  root.now = "other";
  assert.equal(getGroupSelectedDelay(root, data), 10);
});

test("resolves nested and global selections using live members instead of stale records", () => {
  const root = group("root", "auto", [node("auto", [999])]);
  const auto = group("auto", "leaf", [node("leaf", [42])]);
  const data = snapshot([root, auto], { leaf: node("leaf", [900]) });
  assert.equal(getGroupSelectedDelay(root, data), 42);
  assert.equal(getGroupSelectedDelay(data.global, data), 42);
  auto.all = [node("leaf", [75])];
  assert.equal(getGroupSelectedDelay(root, data), 75);
});

test("resolves hidden groups through records and prefers updated member histories", () => {
  const hidden = { ...node("hidden", [999]), all: ["leaf"], now: "leaf" };
  const root = group("root", "hidden", [hidden]);
  const data = snapshot([root], { hidden, leaf: node("leaf", [42]) });
  assert.equal(getGroupSelectedDelay(root, data), 42);
  data.groups.push(group("another", "leaf", [node("leaf", [65])]));
  assert.equal(getGroupSelectedDelay(root, data), 65);
});

test("missing selections, unresolved nodes, and cycles have no selected latency", () => {
  const root = group("root", null, []);
  const data = snapshot([root]);
  assert.equal(getGroupSelectedDelay(root, data), undefined);
  root.now = "missing";
  assert.equal(getGroupSelectedDelay(root, data), undefined);
  root.now = "root";
  assert.equal(getGroupSelectedDelay(root, data), undefined);
  root.now = "nested";
  data.groups.push(group("nested", "root", []));
  assert.equal(getGroupSelectedDelay(root, data), undefined);
});
