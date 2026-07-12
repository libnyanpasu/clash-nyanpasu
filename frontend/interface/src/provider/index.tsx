import type { PropsWithChildren } from 'react'
import {
  MutationCache,
  QueryClient,
  QueryClientProvider,
} from '@tanstack/react-query'
import { extractDegradedRebuild } from '../utils'
import { ClashWSProvider, useClashWSContext } from './clash-ws-provider'
import { MutationProvider } from './mutation-provider'

let degradedRebuildHandler: ((error: string) => void) | null = null

/**
 * Register the app-side notifier for committed-but-degraded rebuilds (PR-4
 * spec §6.3). The interface package owns detection (every mutation result
 * passes through the MutationCache); the app owns presentation (toast + i18n).
 * Returns a disposer so HMR / StrictMode double-mount / tests can unregister
 * (r2, 审计 §六.2).
 */
export const setDegradedRebuildHandler = (handler: (error: string) => void) => {
  degradedRebuildHandler = handler
  return () => {
    if (degradedRebuildHandler === handler) {
      degradedRebuildHandler = null
    }
  }
}

const queryClient = new QueryClient({
  mutationCache: new MutationCache({
    onSuccess: (data) => {
      const error = extractDegradedRebuild(data)
      if (error) {
        degradedRebuildHandler?.(error)
      }
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
