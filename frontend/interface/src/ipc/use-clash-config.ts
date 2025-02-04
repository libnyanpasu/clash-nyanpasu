import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Clash, clash } from '../service/clash'
import { unwrapResult } from '../utils'
import { commands, PatchRuntimeConfig } from './bindings'

/**
 * A hook that manages fetching and updating the Clash configuration.
 *
 * @remarks
 * This hook fetches the current Clash configuration using a query keyed by `['clash-config']`
 * and allows updates via an upsert mutation. The upsert mutation:
 * - First updates the local configuration using `setConfigs`.
 * - Then patches the remote configuration through `commands.patchClashConfig`.
 * - On success, it invalidates the `['clash-config']` query, prompting a refetch to keep the configuration up-to-date.
 *
 * @returns An object with:
 * - `query`: The result of the useQuery hook that retrieves the current configuration.
 * - `upsert`: The mutation object that can be used to update the configuration.
 *
 * @example
 * const { query, upsert } = useClashConfig();
 */
export const useClashConfig = () => {
  const { getConfigs, setConfigs } = clash()

  const queryClient = useQueryClient()

  /**
   * Retrieves the Clash configuration using a query.
   *
   * @remarks
   * The query is configured with the key 'clash-config' and uses the
   * getConfigs function as its query function. This setup ensures that:
   * - The data is uniquely identified and cached based on the query key.
   * - The asynchronous retrieval of configuration data is handled
   *   via the getConfigs function.
   *
   * @see useQuery - For additional configuration options and usage details.
   */
  const query = useQuery({
    queryKey: ['clash-config'],
    queryFn: getConfigs,
  })

  /**
   * Performs an upsert operation to update or insert the Clash configuration.
   *
   * This mutation function accepts a payload that extends both PatchRuntimeConfig and a partial version
   * of Clash.Config. It first updates the local configuration via the setConfigs function, then proceeds
   * to patch the remote configuration with commands.patchClashConfig. On a successful operation, it
   * invalidates the 'clash-config' query to prompt refetching of the newest configuration data.
   *
   * @remarks
   * Ensure that the payload conforms to both the PatchRuntimeConfig specifications and the partial structure
   * of Clash.Config as expected by the remote configuration endpoint.
   *
   * @returns A Promise resolving to the updated configuration, obtained by unwrapping the result of the
   *          commands.patchClashConfig call.
   */
  const upsert = useMutation({
    mutationFn: async (payload: PatchRuntimeConfig & Partial<Clash.Config>) => {
      await setConfigs(payload)

      return unwrapResult(await commands.patchClashConfig(payload))
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['clash-config'] })
    },
  })

  return {
    query,
    upsert,
  }
}
