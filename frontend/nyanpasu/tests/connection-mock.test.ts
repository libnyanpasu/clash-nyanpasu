import { expect, test } from 'vitest'
import {
  mockActiveConnections,
  mockActiveDimensions,
  mockClosedConnections,
} from '../src/pages/(main)/main/connections/_modules/mock-connections.ts'

const now = Date.UTC(2026, 9, 1, 12, 0, 0)

test('active connections are unique and started in the past', () => {
  const active = mockActiveConnections(now)

  expect(active.length).toBeGreaterThan(0)
  expect(new Set(active.map((conn) => conn.id)).size).toBe(active.length)
  for (const conn of active) {
    expect(Date.parse(conn.start)).toBeLessThanOrEqual(now)
    expect(conn.download).toBeGreaterThanOrEqual(0)
    expect(conn.metadata?.host).toBeTruthy()
  }
})

test('closed connections are recent, unique and newest first', () => {
  const closed = mockClosedConnections(now)

  expect(closed.length).toBeGreaterThan(0)
  expect(new Set(closed.map((conn) => conn.id)).size).toBe(closed.length)
  for (const [index, conn] of closed.entries()) {
    expect(conn.closed_at).toBeLessThanOrEqual(now)
    expect(conn.started_at).toBeLessThan(conn.closed_at)
    if (index > 0) {
      expect(conn.closed_at).toBeLessThanOrEqual(closed[index - 1].closed_at)
    }
  }
})

test('an active connection shows up as closed once it ends', () => {
  const later = now + 15 * 60 * 1000
  const stillActive = new Set(mockActiveConnections(later).map((c) => c.id))
  const closedLater = new Set(mockClosedConnections(later).map((c) => c.id))
  const ended = mockActiveConnections(now).filter(
    (conn) => !stillActive.has(conn.id),
  )

  expect(ended.length).toBeGreaterThan(0)
  for (const conn of ended) {
    expect(closedLater.has(conn.id)).toBe(true)
  }
})

test('the same time gives the same connections', () => {
  expect(mockActiveConnections(now)).toEqual(mockActiveConnections(now))
  expect(mockClosedConnections(now)).toEqual(mockClosedConnections(now))
})

test('an active connection has the dimensions it is recorded with on close', () => {
  const later = now + 15 * 60 * 1000
  const closedLater = new Map(
    mockClosedConnections(later).map((conn) => [conn.id, conn.dimensions]),
  )
  const ended = mockActiveConnections(now).filter((conn) =>
    closedLater.has(conn.id),
  )

  expect(ended.length).toBeGreaterThan(0)
  for (const conn of ended) {
    expect(mockActiveDimensions(conn)).toEqual(closedLater.get(conn.id))
  }
})
