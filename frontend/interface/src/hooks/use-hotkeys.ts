import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { mutations, queries } from '../ipc/bindings'
import { invokeMutation, invokeQuery } from '../ipc/query-options'
import { unwrapResult } from '../utils'

export function useHotkeys() {
  const queryClient = useQueryClient()
  const hotkeysQuery = queries.getHotkeys()
  const setHotkeys = mutations.setHotkeys

  const query = useQuery({
    queryKey: hotkeysQuery.queryKey,
    queryFn: async () => {
      const res = await invokeQuery(hotkeysQuery)
      return unwrapResult(res)
    },
  })

  const update = useMutation({
    mutationKey: setHotkeys.mutationKey,
    // Returns the whole MutationOutcome so the MutationCache can see
    // `committed_degraded`: a shortcut the OS refused is still a committed
    // hotkey list, so it stays on the success path. Do not collapse it.
    mutationFn: async (hotkeys: string[]) => {
      return unwrapResult(await invokeMutation(setHotkeys, [hotkeys]))
    },
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: queries.getHotkeys().queryKey,
      })
    },
  })

  return {
    ...query,
    data: query.data ?? [],
    mutate: update.mutateAsync,
  }
}
