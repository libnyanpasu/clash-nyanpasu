import assert from "node:assert/strict";
import test from "node:test";
import { applyClashWsEvent } from "../frontend/interface/src/provider/clash-ws-state.ts";

const snapshot = (sequence = 0) => ({
  sequence,
  state: "connected",
  recording: { connections: true, logs: true, traffic: true, memory: true },
  connections: [],
  logs: [],
  traffic: [],
  memory: [],
});
const log = (sequence, payload = String(sequence)) => ({
  sequence,
  update: { kind: "log_appended", data: { type: "info", time: null, payload } },
});

test("snapshot ordering ignores buffered old events and detects gaps", () => {
  let state = snapshot(10);
  assert.equal(applyClashWsEvent(state, log(9)), state);
  assert.equal(applyClashWsEvent(state, log(10)), state);
  state = applyClashWsEvent(state, log(11));
  assert.equal(state.logs[0].payload, "11");
  assert.equal(applyClashWsEvent(state, log(13)), undefined);
});

test("instance reset replaces history and rejects delayed old snapshots", () => {
  let state = applyClashWsEvent(snapshot(), log(1));
  state = applyClashWsEvent(state, {
    sequence: 20,
    update: { kind: "reset", data: snapshot(20) },
  });
  assert.deepEqual(state.logs, []);
  assert.equal(
    applyClashWsEvent(state, {
      sequence: 5,
      update: { kind: "reset", data: snapshot(5) },
    }),
    state,
  );
  assert.equal(applyClashWsEvent(state, log(19)), state);
});

test("clear precedes subsequent samples, and replaying it does not erase them", () => {
  let state = applyClashWsEvent(snapshot(), log(1));
  const clear = {
    sequence: 2,
    update: { kind: "history_cleared", data: "logs" },
  };
  state = applyClashWsEvent(state, clear);
  state = applyClashWsEvent(state, log(3));
  assert.deepEqual(state.logs.map((item) => item.payload), ["3"]);
  assert.equal(applyClashWsEvent(state, clear), state);
});

test("paused recording advances sequence without appending and history is bounded", () => {
  let state = snapshot();
  for (let sequence = 1; sequence <= 1100; sequence++) {
    state = applyClashWsEvent(state, log(sequence));
  }
  assert.equal(state.logs.length, 1024);
  assert.equal(state.logs[0].payload, "77");
  state = applyClashWsEvent(state, {
    sequence: 1101,
    update: {
      kind: "recording_changed",
      data: { ...state.recording, logs: false },
    },
  });
  state = applyClashWsEvent(state, log(1102));
  assert.equal(state.sequence, 1102);
  assert.equal(state.logs.at(-1).payload, "1100");
});
