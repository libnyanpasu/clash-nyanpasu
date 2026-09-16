import { expect, test } from 'vitest'
import type { ClashWsEvent, ClashWsSnapshot } from '../src/ipc/bindings'
import { applyClashWsEvent } from '../src/provider/clash-ws-state.ts'

const snapshot = (sequence = 0): ClashWsSnapshot => ({
  sequence,
  state: 'connected',
  recording: { connections: true, logs: true, traffic: true, memory: true },
  connections: [],
  logs: [],
  traffic: [],
  memory: [],
})
const log = (sequence: number, payload = String(sequence)): ClashWsEvent => ({
  sequence,
  update: { kind: 'log_appended', data: { type: 'info', time: null, payload } },
})

test('snapshot ordering ignores buffered old events and detects gaps', () => {
  let state = snapshot(10)
  expect(applyClashWsEvent(state, log(9))).toBe(state)
  expect(applyClashWsEvent(state, log(10))).toBe(state)
  state = applyClashWsEvent(state, log(11))!
  expect(state.logs[0].payload).toBe('11')
  expect(applyClashWsEvent(state, log(13))).toBe(undefined)
})

test('instance reset replaces history and rejects delayed old snapshots', () => {
  let state = applyClashWsEvent(snapshot(), log(1))!
  state = applyClashWsEvent(state, {
    sequence: 20,
    update: { kind: 'reset', data: snapshot(20) },
  })!
  expect(state.logs).toEqual([])
  expect(
    applyClashWsEvent(state, {
      sequence: 5,
      update: { kind: 'reset', data: snapshot(5) },
    }),
  ).toBe(state)
  expect(applyClashWsEvent(state, log(19))).toBe(state)
})

test('clear precedes subsequent samples, and replaying it does not erase them', () => {
  let state = applyClashWsEvent(snapshot(), log(1))!
  const clear: ClashWsEvent = {
    sequence: 2,
    update: { kind: 'history_cleared', data: 'logs' },
  }
  state = applyClashWsEvent(state, clear)!
  state = applyClashWsEvent(state, log(3))!
  expect(state.logs.map((item) => item.payload)).toEqual(['3'])
  expect(applyClashWsEvent(state, clear)).toBe(state)
})

test('paused recording advances sequence without appending and history is bounded', () => {
  let state = snapshot()
  for (let sequence = 1; sequence <= 1100; sequence++) {
    state = applyClashWsEvent(state, log(sequence))!
  }
  expect(state.logs.length).toBe(1024)
  expect(state.logs[0].payload).toBe('77')
  state = applyClashWsEvent(state, {
    sequence: 1101,
    update: {
      kind: 'recording_changed',
      data: { ...state.recording, logs: false },
    },
  })!
  state = applyClashWsEvent(state, log(1102))!
  expect(state.sequence).toBe(1102)
  expect(state.logs.at(-1)?.payload).toBe('1100')
})
