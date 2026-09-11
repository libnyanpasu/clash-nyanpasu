import type { LogCursor, LogRow } from './bindings'

export const LOG_CACHE_ROWS = 5_000
export const LOG_CACHE_BYTES = 4 * 1024 * 1024

export function logPosition(row: LogRow): bigint {
  return BigInt(row.id.slice(row.id.lastIndexOf(':') + 1))
}

export function mergeLogRows(
  previous: LogRow[],
  incoming: LogRow[],
  floor: LogCursor | null,
  older = false,
): LogRow[] {
  const rows = new Map(previous.map((row) => [row.id, row]))
  for (const row of incoming) rows.set(row.id, row)
  const ordered = [...rows.values()]
    .filter(
      (row) =>
        !floor ||
        !row.id.startsWith(`${floor.generation}:`) ||
        logPosition(row) >= BigInt(floor.offset),
    )
    .sort((a, b) =>
      logPosition(a) < logPosition(b)
        ? -1
        : logPosition(a) > logPosition(b)
          ? 1
          : 0,
    )
  const retained: LogRow[] = []
  let bytes = 0
  for (const row of older ? ordered : ordered.toReversed()) {
    const size =
      256 +
      2 *
        (row.id.length +
          row.raw.length +
          row.message.length +
          row.target.length)
    if (bytes + size > LOG_CACHE_BYTES || retained.length >= LOG_CACHE_ROWS)
      break
    retained.push(row)
    bytes += size
  }
  return older ? retained : retained.reverse()
}

export function advanceCursor(
  cursor: LogCursor,
  floor: LogCursor | null,
): LogCursor {
  return floor &&
    cursor.generation === floor.generation &&
    BigInt(cursor.offset) < BigInt(floor.offset)
    ? floor
    : cursor
}
