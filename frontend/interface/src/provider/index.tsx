import type { PropsWithChildren } from 'react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { ClashWSProvider } from './clash-ws-provider'

const queryClient = new QueryClient()

export const NyanpasuProvider = ({ children }: PropsWithChildren) => {
  return (
    <QueryClientProvider client={queryClient}>
      <ClashWSProvider>{children}</ClashWSProvider>
    </QueryClientProvider>
  )
}
