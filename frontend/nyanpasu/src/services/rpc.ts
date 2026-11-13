import { createQueryBindings, shouldRetryRpcQuery } from '@nyanpasu/query'
import { createRpcClient } from '@nyanpasu/rpc'
import { QueryClient } from '@tanstack/react-query'

// The application composition root owns this webview's clients and adapters.
export const rpc = createRpcClient()
export const { queries, mutations } = createQueryBindings(rpc)
export const commands = rpc
export const events = rpc.events
export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: shouldRetryRpcQuery,
    },
  },
})

if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    rpc.dispose()
    queryClient.clear()
  })
}
