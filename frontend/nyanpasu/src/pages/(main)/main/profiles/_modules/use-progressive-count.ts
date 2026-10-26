import { useEffect, useState, useTransition } from 'react'

/** Cards per transition: about the rows a window shows at once. */
const BATCH = 6

/**
 * How many of `total` cards to render. Mounting every card in the router's
 * synchronous update makes opening a page one long task, so the page commits
 * without them and each transition after adds a batch, top first, with a frame
 * painted in between, until all show.
 */
export function useProgressiveCount(total: number) {
  const [count, setCount] = useState(0)
  const [, startTransition] = useTransition()

  useEffect(() => {
    if (count < total) {
      startTransition(() => setCount(count + BATCH))
    }
  }, [count, total])

  return Math.min(count, total)
}
