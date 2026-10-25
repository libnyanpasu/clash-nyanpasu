import { useEffect, useReducer } from 'react'
import { profileContentSummary } from './profile-content-summary'
import SummaryWorker from './profile-content-summary.worker?worker'

type Summary = ReturnType<typeof profileContentSummary>

type Entry = {
  content: string
  summary: Promise<Summary>
  settled?: { summary: Summary }
}

// The last summary of each profile, kept for the session, so a page opened
// again shows it at once. A profile's file is parsed again only when its text
// differs from the one summarized.
const entries = new Map<string, Entry>()

let worker: Worker | undefined
let nextId = 0
const waiting = new Map<number, (summary: Summary) => void>()

function summarize(content: string) {
  if (!worker) {
    worker = new SummaryWorker()
    worker.onmessage = ({
      data,
    }: MessageEvent<{ id: number; summary: Summary }>) => {
      waiting.get(data.id)?.(data.summary)
      waiting.delete(data.id)
    }
    // A worker that cannot run leaves the counts unknown.
    worker.onerror = () => {
      waiting.forEach((resolve) => resolve(null))
      waiting.clear()
    }
  }

  const id = (nextId += 1)
  worker.postMessage({ id, content })

  return new Promise<Summary>((resolve) => waiting.set(id, resolve))
}

function entryFor(uid: string, content: string) {
  const current = entries.get(uid)
  if (current?.content === content) {
    return current
  }

  const entry: Entry = { content, summary: summarize(content) }
  entry.summary.then((summary) => {
    entry.settled = { summary }
  })
  entries.set(uid, entry)

  return entry
}

/**
 * The counts of a profile's file, parsed in a worker. `undefined` while the
 * file or its summary loads; `null` when the file has no countable shape.
 */
export function useProfileContentSummary(
  uid: string,
  content: string | undefined,
): Summary | undefined {
  const [, update] = useReducer((count: number) => count + 1, 0)

  useEffect(() => {
    if (content === undefined) {
      return
    }

    const entry = entryFor(uid, content)
    if (entry.settled) {
      return
    }

    let current = true
    entry.summary.then(() => current && update())

    return () => {
      current = false
    }
  }, [uid, content])

  const entry = entries.get(uid)

  return content !== undefined && entry?.content === content
    ? entry.settled?.summary
    : undefined
}
