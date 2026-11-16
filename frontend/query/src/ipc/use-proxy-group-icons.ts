import { useQuery } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { unwrapQueryOptions } from './query-options'

const NO_ICONS: Readonly<Record<string, string>> = Object.freeze({})

/**
 * The icon of each proxy group, by group name.
 *
 * Icons are profile-provided URLs, so a plain `read` caller never receives
 * them with the proxies; this reads the protected listing. A caller that is
 * refused simply has no icons, and the group is drawn without one.
 */
export const useProxyGroupIcons = () => {
  const api = useQueryApi()
  const options = api.queries.getProxyGroupIcons()

  const query = useQuery({
    ...unwrapQueryOptions(options, options.queryFn!),
    retry: false,
  })

  return query.data ?? NO_ICONS
}
