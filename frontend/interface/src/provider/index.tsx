import type { PropsWithChildren } from 'react'
import {
  MutationCache,
  QueryClient,
  QueryClientProvider,
} from '@tanstack/react-query'
import type { Degradation } from '../ipc/bindings'
import { ClashWSProvider, useClashWSContext } from './clash-ws-provider'
import { MutationProvider } from './mutation-provider'

let mutationDegradationHandler: ((degradations: Degradation[]) => void) | null =
  null

/**
 * Register the app-side notifier for committed-degraded mutation outcomes
 * (PR-4S S08 / plan §12). The interface package owns detection (every mutation
 * result passes through the MutationCache); the app owns presentation
 * (toast + i18n). Returns a disposer so HMR / StrictMode double-mount / tests
 * can unregister.
 */
export const setMutationDegradationHandler = (
  handler: (degradations: Degradation[]) => void,
) => {
  mutationDegradationHandler = handler
  return () => {
    if (mutationDegradationHandler === handler) {
      mutationDegradationHandler = null
    }
  }
}

const queryClient = new QueryClient({
  mutationCache: new MutationCache({
    onSuccess: (data) => {
      // Only the final S08 wire is recognized. Applied / hard-error / legacy
      // rebuild shapes are ignored so dual-wire handling cannot linger.
      if (
        !data ||
        typeof data !== 'object' ||
        !('status' in data) ||
        (data as { status?: unknown }).status !== 'committed_degraded'
      ) {
        return
      }

      const degradations = (data as { degradations?: unknown }).degradations
      if (!Array.isArray(degradations)) {
        return
      }

      // Still on the mutation success path: callers' onSuccess / invalidation
      // continue to run for committed state even when side effects degraded.
      mutationDegradationHandler?.(degradations as Degradation[])
    },
  }),
})

export const NyanpasuProvider = ({ children }: PropsWithChildren) => {
  return (
    <QueryClientProvider client={queryClient}>
      <MutationProvider>
        <ClashWSProvider>{children}</ClashWSProvider>
      </MutationProvider>
    </QueryClientProvider>
  )
}

export { useClashWSContext }
