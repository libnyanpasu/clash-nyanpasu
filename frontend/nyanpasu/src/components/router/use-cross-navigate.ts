import { useCallback, useState } from 'react'
import {
  useLocation,
  useRouter,
  type NavigateOptions,
  type RegisteredRouter,
} from '@tanstack/react-router'
import type { CrossPage } from './cross-navigation'

type CrossJump<TFrom extends string, TTo extends string | undefined> = {
  from: CrossPage
  /** Written into the origin entry, so returning scrolls back to it. */
  originFocus?: string
  /** Written into the new entry: what the target page scrolls to. */
  targetFocus?: string
  /** Typed TanStack navigate options for the target (`to`, `search`). */
  to: NavigateOptions<RegisteredRouter, TFrom, TTo>
}

/** Jumps to another page, leaving it a return ticket to this entry. */
export function useCrossNavigate() {
  const router = useRouter()

  return useCallback(
    async <TTo extends string | undefined, TFrom extends string = string>({
      from,
      originFocus,
      targetFocus,
      to,
    }: CrossJump<TFrom, TTo>) => {
      // Replacing keeps the origin's history index, which the ticket points at.
      if (originFocus != null) {
        await router.navigate({
          to: '.',
          search: true,
          replace: true,
          resetScroll: false,
          state: (prev) => ({ ...prev, focus: originFocus }),
        })
      }

      const { href, state } = router.history.location

      // The caller's options were checked against its own `to`; spreading the
      // generic options into `navigate` defeats its inference.
      await router.navigate({
        ...(to as NavigateOptions),
        state: {
          returnTo: { page: from, href, index: state.__TSR_index },
          focus: targetFocus,
        },
      })
    },
    [router],
  )
}

/** The page's focus for this entry, read once when the page mounts. */
export function useEntryFocus(): string | undefined {
  const focus = useLocation({ select: (location) => location.state.focus })

  return useState(focus)[0]
}
