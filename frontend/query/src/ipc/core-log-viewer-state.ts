import type {
  CoreLogCursor,
  CoreLogRow,
  CoreLogStatus,
} from '@nyanpasu/rpc/types'

export const MAX_CORE_LOG_ROWS = 500
export const MAX_CORE_LOG_BYTES = 2 * 1024 * 1024

export const compareCoreLogCursors = (a: CoreLogCursor, b: CoreLogCursor) =>
  a.sequence - b.sequence

export const coreLogRowKey = (cursor: CoreLogCursor) =>
  `${cursor.generation}:${cursor.sequence}`

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

/** Session changes and cache limits apply even to an idle page. */
export function mergeCoreLogRows(
  current: CoreLogRow[],
  incoming: CoreLogRow[],
  status: CoreLogStatus,
  older = false,
) {
  // Both the backend pages and retained previews are ordered by sequence.
  const retained = (row: CoreLogRow) =>
    row.id.generation === status.generation &&
    status.first !== null &&
    status.head !== null &&
    row.id.sequence >= status.first.sequence &&
    row.id.sequence <= status.head.sequence
  const left = current.filter(retained)
  const right = incoming.filter(retained)
  const rows: CoreLogRow[] = []
  let a = 0
  let b = 0
  while (a < left.length || b < right.length) {
    if (
      b === right.length ||
      (a < left.length && left[a].id.sequence <= right[b].id.sequence)
    ) {
      const row = left[a++]
      if (b < right.length && row.id.sequence === right[b].id.sequence) b++
      rows.push(row)
    } else rows.push(right[b++])
  }
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
