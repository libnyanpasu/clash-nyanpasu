import type { TrayIcon } from '@nyanpasu/rpc/types'
import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { unwrapQueryOptions } from './query-options'

export const useCachedIcon = (url: string | null) => {
  const api = useQueryApi()
  const options = api.queries.getCachedIcon(url ?? '')

  return useQuery({
    ...unwrapQueryOptions(options, options.queryFn!),
    enabled: url !== null,
  })
}

export const useTrayIcon = (mode: TrayIcon, version?: number) => {
  const api = useQueryApi()
  const options = api.queries.getTrayIcon(mode)

  return useQuery({
    ...unwrapQueryOptions(options, options.queryFn!),
    queryKey: [...options.queryKey, version],
  })
}
