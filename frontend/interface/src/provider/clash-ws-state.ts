import type { ClashWsEvent, ClashWsSnapshot } from '../ipc/bindings'

export const MAX_CONNECTIONS_HISTORY = 32
export const MAX_MEMORY_HISTORY = 32
export const MAX_TRAFFIC_HISTORY = 32
export const MAX_LOGS_HISTORY = 1024

const append = <T>(items: T[], item: T, limit: number) =>
  [...items, item].slice(-limit)

// undefined requests a snapshot after a delivery gap. Reset snapshots may jump
// over missed events; older snapshots/events can never replace newer state.
export function applyClashWsEvent(
  current: ClashWsSnapshot,
  event: ClashWsEvent,
): ClashWsSnapshot | undefined {
  if (event.sequence <= current.sequence) return current
  const { update } = event
  if (update.kind === 'reset') return update.data
  if (event.sequence !== current.sequence + 1) return undefined
  const next = { ...current, sequence: event.sequence }
  switch (update.kind) {
    case 'state_changed':
      return { ...next, state: update.data }
    case 'recording_changed':
      return { ...next, recording: update.data }
    case 'history_cleared':
      return { ...next, [update.data]: [] }
    case 'connections_updated':
      return next.recording.connections
        ? {
            ...next,
            connections: append(
              next.connections,
              update.data,
              MAX_CONNECTIONS_HISTORY,
            ),
          }
        : next
    case 'log_appended':
      return next.recording.logs
        ? { ...next, logs: append(next.logs, update.data, MAX_LOGS_HISTORY) }
        : next
    case 'traffic_updated':
      return next.recording.traffic
        ? {
            ...next,
            traffic: append(next.traffic, update.data, MAX_TRAFFIC_HISTORY),
          }
        : next
    case 'memory_updated':
      return next.recording.memory
        ? {
            ...next,
            memory: append(next.memory, update.data, MAX_MEMORY_HISTORY),
          }
        : next
  }
}
