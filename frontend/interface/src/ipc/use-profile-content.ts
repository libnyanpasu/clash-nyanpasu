import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import { commands } from './bindings'

/**
 * A custom hook that manages profile content data fetching and updating.
 *
 * @remarks
 * This hook provides functionality to read and write profile content using React Query.
 * It includes both query and mutation capabilities for profile data management.
 *
 * @param uid - The unique identifier for the profile
 *
 * @returns An object containing:
 * - `query` - The React Query result object for fetching profile content
 * - `upsert` - Mutation object for saving/updating profile content
 *
 * @example
 * ```tsx
 * const { query, upsert } = useProfileContent("user123");
 * const { data, isLoading } = query;
 *
 * // To update profile content
 * upsert.mutate("new profile content");
 * ```
 */
export const useProfileContent = (uid: string) => {
  const queryClient = useQueryClient()

  /**
   * A React Query hook that fetches profile content based on a user ID.
   *
   * @remarks
   * This query uses the `readProfileFile` command to retrieve profile data
   * and unwraps the result.
   *
   * @param uid - The user ID used to fetch the profile content
   * @returns A React Query result object containing the profile content data,
   * loading state, and error state
   *
   * @example
   * ```tsx
   * const { data, isLoading } = useQuery(['profileContent', userId]);
   * ```
   */
  const query = useQuery({
    queryKey: ['profile-content', uid],
    queryFn: async () => {
      return unwrapResult(await commands.readProfileFile(uid))
    },
    enabled: !!uid,
  })

  /**
   * Mutation hook for saving and updating profile file data
   *
   * @remarks
   * This mutation will invalidate the profile content query cache on success
   *
   * @example
   * ```ts
   * const { mutate } = upsert;
   * mutate("profile content");
   * ```
   *
   * @returns A mutation object that handles saving profile file data
   */
  const upsert = useMutation({
    mutationFn: async (fileData: string) => {
      return unwrapResult(await commands.saveProfileFile(uid, fileData))
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['profileContent', uid] })
    },
  })

  return {
    query,
    upsert,
  }
}
