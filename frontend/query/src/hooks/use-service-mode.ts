import { unwrapResult } from '@nyanpasu/rpc'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { invokeMutation } from '../ipc/query-options'
import { useSetting } from '../ipc/use-settings'
import { useQueryApi } from '../provider/rpc-provider'

/** Writes service mode through its dedicated command and waits for saved values. */
export function useServiceMode() {
  const api = useQueryApi()
  const queryClient = useQueryClient()
  const setting = useSetting('enable_service_mode')
  const command = api.mutations.setServiceMode

  const update = useMutation({
    mutationKey: command.mutationKey,
    // Keep the whole outcome so committed_degraded remains a committed result.
    mutationFn: async (enabled: boolean) =>
      unwrapResult(await invokeMutation(command, [enabled])),
    // Wait for saved values before allowing the next toggle.
    onSuccess: () =>
      queryClient.invalidateQueries({
        queryKey: api.queries.getAppConfig().queryKey,
      }),
  })

  return {
    value: setting.value,
    mutate: update.mutateAsync,
    isPending: update.isPending,
  }
}
