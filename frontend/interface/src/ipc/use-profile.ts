import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { unwrapResult } from '../utils'
import {
  commands,
  Profile,
  type ProfileBuilder,
  type ProfilesBuilder,
} from './bindings'

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

type ProfileHelperFn = {
  view: () => Promise<null | undefined>
  update: (profile: ProfileBuilder) => Promise<null | undefined>
  drop: () => Promise<null | undefined>
}

export type ProfileQueryResult = NonNullable<
  ReturnType<typeof useProfile>['query']['data']
>

export type ProfileQueryResultItem = Profile & Partial<ProfileHelperFn>
/**
 * A custom hook for managing profiles with various operations including creation, updating, sorting, and deletion.
 *
 * @remarks
 * This hook provides comprehensive profile management functionality through React Query:
 * - Fetching profiles with optional helper functions
 * - Creating/importing profiles from URLs or files
 * - Updating existing profiles
 * - Reordering profiles
 * - Upserting profile configurations
 * - Deleting profiles
 *
 * Each operation automatically handles cache invalidation and refetching when successful.
 *
 * @param options - Configuration options for the hook
 * @param options.without_helper_fn - When true, disables the addition of helper functions to profile items
 *
 * @returns An object containing:
 * - query: Query result for fetching profiles
 * - create: Mutation for creating/importing profiles
 * - update: Mutation for updating existing profiles
 * - sort: Mutation for reordering profiles
 * - upsert: Mutation for upserting profile configurations
 * - drop: Mutation for deleting profiles
 *
 * @example
 * ```tsx
 * const { query, create, update, sort, upsert, drop } = useProfile();
 *
 * // Fetch profiles
 * const profiles = query.data?.items;
 *
 * // Create a new profile
 * create.mutate({ type: 'file', data: { item: newProfile, fileData: 'config' }});
 *
 * // Update a profile
 * update.mutate({ uid: 'profile-id', profile: updatedProfile });
 * ```
 */
export const useProfile = (options?: { without_helper_fn?: boolean }) => {
  const queryClient = useQueryClient()

  function addHelperFn(item: Profile): Profile & ProfileHelperFn {
    return {
      ...item,
      view: async () => unwrapResult(await commands.viewProfile(item.uid)),
      update: async (profile: ProfileBuilder) =>
        await update.mutateAsync({ uid: item.uid, profile }),
      drop: async () => await drop.mutateAsync(item.uid),
    }
  }

  /**
   * Retrieves and processes a list of profiles.
   *
   * This query uses the `useQuery` hook to fetch profile data by invoking the `commands.getProfiles()` command.
   * The raw result is first unwrapped using `unwrapResult`, and then each profile item is augmented with additional
   * helper functions:
   *
   * - view: Invokes `commands.viewProfile` with the profile's UID.
   * - update: Executes the update mutation by passing an object containing the UID and the new profile data.
   * - drop: Executes the drop mutation using the profile's UID.
   *
   * @returns A promise resolving to an object containing the profile list along with the extended helper functions.
   */
  const query = useQuery({
    queryKey: ['profiles'],
    queryFn: async () => {
      const result = unwrapResult(await commands.getProfiles())

      // Skip helper functions if without_helper_fn is set
      if (options?.without_helper_fn) {
        return result
      }

      return {
        ...result,
        items: result?.items?.map((item) => {
          return addHelperFn(item)
        }),
      }
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
   * @param profile - The profile data of type ProfileBuilder to update with
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
      profile: ProfileBuilder
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
    mutationFn: async (options: Partial<ProfilesBuilder>) => {
      return unwrapResult(
        await commands.patchProfilesConfig(options as ProfilesBuilder),
      )
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
