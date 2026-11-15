import { clearRevokedPrincipalState } from '@/components/auth/principal-marker'
import { createQueryBindings, shouldRetryRpcQuery } from '@nyanpasu/query'
import { createDefaultRpcTransport, createRpcClient } from '@nyanpasu/rpc'
import { QueryClient } from '@tanstack/react-query'

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: shouldRetryRpcQuery,
    },
  },
})

// The application composition root owns this webview's clients and adapters.
export const rpc = createRpcClient(
  createDefaultRpcTransport(undefined, {
    onUnauthorized: () => {
      rpc.dispose()
      clearRevokedPrincipalState(queryClient)
      window.location.replace('/login')
    },
  }),
)
export const { queries, mutations } = createQueryBindings(rpc)
export const commands = rpc
export const events = rpc.events

if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    rpc.dispose()
    queryClient.clear()
  })
}
