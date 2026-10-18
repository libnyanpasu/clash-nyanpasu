import { unwrapResult } from '@nyanpasu/rpc'
import { skipToken, useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'

/** Release notes of the versions after `from` up to `to`; idle until `to` is known. */
export const useAppUpdateChangelog = (from: string, to: string | undefined) => {
  const api = useQueryApi()

  return useQuery({
    queryKey: ['getAppUpdateChangelog', from, to],
    queryFn:
      to == null
        ? skipToken
        : async () => unwrapResult(await api.getAppUpdateChangelog(from, to)),
    // Published notes rarely change; reopening the dialog should not refetch.
    staleTime: Infinity,
    retry: false,
  })
}
