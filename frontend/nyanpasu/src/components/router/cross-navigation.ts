import type { HistoryLocation, HistoryState } from '@tanstack/react-router'

export type CrossPage = 'rules' | 'connections' | 'traffic'

export type ReturnTicket = {
  page: CrossPage
  /** The origin entry's href, used when that entry is no longer behind this one. */
  href: string
  /** The origin entry's `__TSR_index`. */
  index: number
}

declare module '@tanstack/react-router' {
  interface HistoryState {
    returnTo?: ReturnTicket
    /** The item the page scrolls to and highlights when it shows this entry. */
    focus?: string
  }
}

type ParsedHistoryState = HistoryLocation['state']

export type ReturnStep =
  { kind: 'go'; delta: number } | { kind: 'href'; href: string }

export function returnStep(
  ticket: ReturnTicket,
  currentIndex: number,
): ReturnStep {
  const delta = ticket.index - currentIndex

  return delta < 0 ? { kind: 'go', delta } : { kind: 'href', href: ticket.href }
}

/** State for navigations within a page: the way back stays, a focus does not. */
export function keepReturn(previous: ParsedHistoryState): HistoryState {
  return { returnTo: previous.returnTo }
}

/** -1 when the history index went down (a step back), else undefined. */
export function historyDirection(
  previousIndex: number,
  nextIndex: number,
): -1 | undefined {
  return nextIndex < previousIndex ? -1 : undefined
}
