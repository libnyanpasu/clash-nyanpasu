import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { commands } from '../ipc/bindings'
import { unwrapResult } from '../utils'

const HOTKEYS_QUERY_KEY = 'hotkeys'

export function useHotkeys() {
  const queryClient = useQueryClient()

  const query = useQuery({
    queryKey: [HOTKEYS_QUERY_KEY],
    queryFn: async () => {
      const res = await commands.getHotkeys()
      return unwrapResult(res) ?? []
    },
  })

  const update = useMutation({
    mutationFn: async (hotkeys: string[]) => {
      return unwrapResult(await commands.setHotkeys(hotkeys))
    },
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: [HOTKEYS_QUERY_KEY],
      })
    },
  })

  return {
    ...query,
    data: query.data ?? [],
    mutate: update.mutateAsync,
  }
}
