import type { CoreLogCursor, CoreLogRow, CoreLogStatus } from './rpc-bindings'

export const MAX_CORE_LOG_ROWS = 500
export const MAX_CORE_LOG_BYTES = 2 * 1024 * 1024

export const compareCoreLogCursors = (a: CoreLogCursor, b: CoreLogCursor) =>
  a.segment - b.segment || a.sequence - b.sequence

export const coreLogRowKey = (cursor: CoreLogCursor) =>
  `${cursor.generation}:${cursor.segment}:${cursor.sequence}`

export const estimateCoreLogRowBytes = (row: CoreLogRow) =>
  256 +
  2 *
    (row.id.generation.length +
      row.record.payload.length +
      row.record.type.length +
      (row.record.time?.length ?? 0) +
      row.record.source.capture.length +
      row.record.source.instance_id.length +
      (row.record.source.core_kind?.length ?? 0))

/** Both the rolling boundary and the cache limits apply, even to an idle page. */
export function mergeCoreLogRows(
  current: CoreLogRow[],
  incoming: CoreLogRow[],
  status: CoreLogStatus,
  older = false,
) {
  const merged = new Map<string, CoreLogRow>()
  for (const row of [...current, ...incoming]) {
    if (
      row.id.generation !== status.generation ||
      !status.first ||
      !status.head ||
      compareCoreLogCursors(row.id, status.first) < 0 ||
      compareCoreLogCursors(row.id, status.head) > 0
    )
      continue
    merged.set(coreLogRowKey(row.id), row)
  }
  const rows = [...merged.values()].sort((a, b) =>
    compareCoreLogCursors(a.id, b.id),
  )
  let bytes = rows.reduce(
    (total, row) => total + estimateCoreLogRowBytes(row),
    0,
  )
  let first = 0
  let last = rows.length
  while (
    last > first &&
    (last - first > MAX_CORE_LOG_ROWS || bytes > MAX_CORE_LOG_BYTES)
  ) {
    bytes -= estimateCoreLogRowBytes(rows[older ? --last : first++])
  }
  const bounded = rows.slice(first, last)

  return bounded.length === current.length &&
    bounded.every((row, index) => row === current[index])
    ? current
    : bounded
}
