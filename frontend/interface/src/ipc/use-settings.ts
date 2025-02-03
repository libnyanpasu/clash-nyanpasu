import { merge } from 'lodash-es'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import { commands, type IVerge } from './bindings'

/**
 * Custom hook for managing Verge configuration settings using React Query.
 * Provides functionality to fetch and update settings with automatic cache invalidation.
 *
 * @returns An object containing:
 * - query: UseQueryResult for fetching settings
 *   - data: Current Verge configuration
 *   - status: Query status ('loading', 'error', 'success')
 *   - error: Error object if query fails
 * - upsert: UseMutationResult for updating settings
 *   - mutate: Function to update configuration
 *   - status: Mutation status
 *
 * @example
 * ```tsx
 * const { query, upsert } = useSettings();
 *
 * // Get current settings
 * const settings = query.data;
 *
 * // Update settings
 * upsert.mutate({ theme: 'dark' });
 * ```
 */
export const useSettings = () => {
  const queryClient = useQueryClient()

  /**
   * A query hook that fetches Verge configuration settings.
   * Uses React Query to manage the data fetching state.
   *
   * @returns UseQueryResult containing:
   * - data: The unwrapped Verge configuration data
   * - status: Current status of the query ('loading', 'error', 'success')
   * - error: Error object if the query fails
   * - other standard React Query properties
   */
  const query = useQuery({
    queryKey: ['settings'],
    queryFn: async () => {
      return unwrapResult(await commands.getVergeConfig())
    },
  })

  /**
   * Mutation hook for updating Verge configuration settings
   *
   * @remarks
   * Uses React Query's useMutation to manage state and side effects
   *
   * @param options - Partial configuration options to update
   * @returns Mutation object containing mutate function and mutation state
   *
   * @example
   * ```ts
   * const { mutate } = upsert();
   * mutate({ theme: 'dark' });
   * ```
   */
  const upsert = useMutation({
    // Partial to allow for partial updates
    mutationFn: async (options: Partial<IVerge>) => {
      return unwrapResult(await commands.patchVergeConfig(options as IVerge))
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['settings'] })
    },
  })

  return {
    query,
    upsert,
  }
}

/**
 * A custom hook that manages a specific setting from the Verge configuration.
 *
 * @template K - The key type extending keyof IVerge
 * @param key - The specific setting key to manage
 * @returns An object containing:
 * - value: The current value of the specified setting
 * - upsert: Function to update the setting value
 * - Additional merged hook status properties
 *
 * @example
 * ```typescript
 * const { value, upsert } = useSetting('theme');
 * // value contains current theme setting
 * // upsert can be used to update theme setting
 * ```
 */
export const useSetting = <K extends keyof IVerge>(key: K) => {
  const {
    query: { data, ...query },
    upsert: update,
  } = useSettings()

  /**
   * The value retrieved from the data object using the specified key.
   * May be undefined if either data is undefined or the key doesn't exist in data.
   */
  const value = data?.[key]

  /**
   * Updates a specific setting value in the Verge configuration
   * @param value - The new value to be set for the specified key
   * @returns void
   * @remarks This function will not execute if the data is not available
   */
  const upsert = async (value: IVerge[K]) => {
    if (!data) {
      return
    }

    await update.mutateAsync({ [key]: value })
  }

  return {
    value,
    upsert,
    // merge hook status
    ...merge(query, update),
  }
}
