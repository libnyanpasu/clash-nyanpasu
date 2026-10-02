import type { LogCursor, LogRow } from '@nyanpasu/rpc/types'

export const LOG_CACHE_ROWS = 5_000
export const LOG_CACHE_BYTES = 4 * 1024 * 1024

export function logPosition(row: LogRow): bigint {
  return BigInt(row.id.slice(row.id.lastIndexOf(':') + 1))
}

function isAscending(rows: LogRow[]) {
  for (let i = 1; i < rows.length; i++)
    if (logPosition(rows[i - 1]) >= logPosition(rows[i])) return false
  return true
}

/** `previous` is an earlier result, so it is already in position order. */
export function mergeLogRows(
  previous: LogRow[],
  incoming: LogRow[],
  floor: LogCursor | null,
  older = false,
): LogRow[] {
  const kept = incoming.filter(
    (row) =>
      !floor ||
      !row.id.startsWith(`${floor.generation}:`) ||
      logPosition(row) >= BigInt(floor.offset),
  )
  // Cached rows already passed the floor and the budget, so a page with
  // nothing new, such as an idle tail poll, keeps the array.
  if (!kept.length) return previous
  let ordered: LogRow[]
  // Tail and history pages arrive in order past one end of the cache, so
  // they are spliced on rather than re-sorting the cache every poll.
  if (
    isAscending(kept) &&
    (!previous.length ||
      (older
        ? logPosition(kept[kept.length - 1]) < logPosition(previous[0])
        : logPosition(kept[0]) > logPosition(previous[previous.length - 1])))
  ) {
    ordered = older ? kept.concat(previous) : previous.concat(kept)
  } else {
    const rows = new Map(previous.map((row) => [row.id, row]))
    for (const row of kept) rows.set(row.id, row)
    ordered = [...rows.values()]
      .map((row) => ({ row, position: logPosition(row) }))
      .sort((a, b) =>
        a.position < b.position ? -1 : a.position > b.position ? 1 : 0,
      )
      .map(({ row }) => row)
  }
  let bytes = 0
  let count = 0
  while (count < ordered.length && count < LOG_CACHE_ROWS) {
    const row = ordered[older ? count : ordered.length - 1 - count]
    const size =
      256 +
      2 *
        (row.id.length +
          row.raw.length +
          row.message.length +
          row.target.length)
    if (bytes + size > LOG_CACHE_BYTES) break
    bytes += size
    count += 1
  }
  return older ? ordered.slice(0, count) : ordered.slice(ordered.length - count)
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
