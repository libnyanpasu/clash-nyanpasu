import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import { commands, ProfileKind, ProfilesBuilder } from './bindings'

type URLImportParams = Parameters<typeof commands.importProfile>

type ManualImportParams = Parameters<typeof commands.createProfile>

type CreateParams =
  | {
      type: 'url'
      data: {
        url: URLImportParams[0]
        option: URLImportParams[1]
      }
    }
  | {
      type: 'manual'
      data: {
        item: ManualImportParams[0]
        fileData: ManualImportParams[1]
      }
    }

/**
 * A custom hook for managing profile operations using React Query.
 * Provides functionality for CRUD operations on profiles including creation,
 * updating, reordering, and deletion.
 *
 * @returns An object containing:
 * - query: {@link UseQueryResult} Hook result for fetching profiles data
 * - create: {@link UseMutationResult} Mutation for creating/importing profiles
 * - update: {@link UseMutationResult} Mutation for updating existing profiles
 * - sort: {@link UseMutationResult} Mutation for reordering profiles
 * - upsert: {@link UseMutationResult} Mutation for upserting profile configurations
 * - drop: {@link UseMutationResult} Mutation for deleting profiles
 *
 * @example
 * ```typescript
 * const { query, create, update, sort, upsert, drop } = useProfile();
 *
 * // Fetch profiles
 * const { data, isLoading } = query;
 *
 * // Create a new profile
 * create.mutate({
 *   type: 'file',
 *   data: { item: profileData, fileData: 'config' }
 * });
 *
 * // Update a profile
 * update.mutate({ uid: 'profile-id', profile: updatedProfile });
 *
 * // Reorder profiles
 * sort.mutate(['uid1', 'uid2', 'uid3']);
 *
 * // Upsert profile config
 * upsert.mutate(profilesConfig);
 *
 * // Delete a profile
 * drop.mutate('profile-id');
 * ```
 */
export const useProfile = () => {
  const queryClient = useQueryClient()

  /**
   * A React Query hook that fetches profiles data.
   * data is the full Profile configuration, including current, chain, valid, and items fields
   * Uses the `getProfiles` command to retrieve profile information.
   *
   * @returns {UseQueryResult} A query result object containing:
   * - data: {
   *     current: string | null     - Currently selected profile UID
   *     chain: string[]            - Global chain of profile UIDs
   *     valid: boolean             - Whether the profile configuration is valid
   *     items: Profile[]           - Array of profile configurations
   *   }
   * - `isLoading`: Boolean indicating if the query is in loading state
   * - `error`: Error object if the query failed
   * - Other standard React Query result properties
   */
  const query = useQuery({
    queryKey: ['profiles'],
    queryFn: async () => {
      return unwrapResult(await commands.getProfiles())
    },
  })

  /**
   * Mutation hook for creating or importing profiles
   *
   * @remarks
   * This mutation handles two types of profile creation:
   * 1. URL-based import using `importProfile` command
   * 2. Direct creation using `createProfile` command
   *
   * @returns A mutation object that accepts CreateParams and handles profile creation
   *
   * @throws Will throw an error if the profile creation/import fails
   *
   * @example
   * ```ts
   * const { mutate } = create();
   * // Import from URL
   * mutate({ type: 'url', data: { url: 'https://example.com/config.yaml', option: {...} }});
   * // Create directly
   * mutate({ type: 'file', data: { item: {...}, fileData: '...' }});
   * ```
   */
  const create = useMutation({
    mutationFn: async ({ type, data }: CreateParams) => {
      if (type === 'url') {
        const { url, option } = data
        return unwrapResult(await commands.importProfile(url, option))
      } else {
        const { item, fileData } = data
        return unwrapResult(await commands.createProfile(item, fileData))
      }
    },
    onSuccess: () => {
      // Invalidate and refetch
      queryClient.invalidateQueries({ queryKey: ['profiles'] })
    },
  })

  /**
   * Mutation hook for updating a profile.
   * Uses React Query's useMutation to handle profile updates.
   *
   * @remarks
   * This mutation will automatically invalidate and refetch the 'profiles' query on success
   *
   * @param uid - The unique identifier of the profile to update
   * @param profile - The profile data of type ProfileKind to update with
   *
   * @returns A mutation object containing mutate function and mutation state
   *
   * @throws Will throw an error if the profile update fails
   */
  const update = useMutation({
    mutationFn: async ({
      uid,
      profile,
    }: {
      uid: string
      profile: ProfileKind
    }) => {
      return unwrapResult(await commands.patchProfile(uid, profile))
    },
    onSuccess: () => {
      // Invalidate and refetch
      queryClient.invalidateQueries({ queryKey: ['profiles'] })
    },
  })

  /**
   * Mutation hook for reordering profiles.
   * Uses the React Query's useMutation hook to handle profile reordering operations.
   *
   * @remarks
   * This mutation takes an array of profile UIDs and reorders them according to the new sequence.
   * On successful reordering, it invalidates the 'profiles' query cache to trigger a refresh.
   *
   * @example
   * ```typescript
   * const { mutate } = sort;
   * mutate(['uid1', 'uid2', 'uid3']);
   * ```
   */
  const sort = useMutation({
    mutationFn: async (uids: string[]) => {
      return unwrapResult(await commands.reorderProfilesByList(uids))
    },
    onSuccess: () => {
      // Invalidate and refetch
      queryClient.invalidateQueries({ queryKey: ['profiles'] })
    },
  })

  /**
   * Mutation hook for upserting profile configurations.
   *
   * @remarks
   * This mutation handles the update/insert of profile configurations and invalidates
   * the profiles query cache on success.
   *
   * @returns A mutation object that:
   * - Accepts a ProfilesBuilder parameter for the mutation
   * - Returns the unwrapped result from patchProfilesConfig command
   * - Automatically invalidates the 'profiles' query cache on successful mutation
   */
  const upsert = useMutation({
    mutationFn: async (options: ProfilesBuilder) => {
      return unwrapResult(await commands.patchProfilesConfig(options))
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['profiles'] })
    },
  })

  /**
   * A mutation hook for deleting a profile.
   *
   * @returns {UseMutationResult} A mutation object that:
   * - Accepts a profile UID as parameter
   * - Deletes the profile via commands.deleteProfile
   * - Automatically invalidates 'profiles' queries on success
   */
  const drop = useMutation({
    mutationFn: async (uid: string) => {
      return unwrapResult(await commands.deleteProfile(uid))
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['profiles'] })
    },
  })

  return {
    query,
    create,
    update,
    sort,
    upsert,
    drop,
  }
}
