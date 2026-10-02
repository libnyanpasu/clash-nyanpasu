import { unwrapResult } from '@nyanpasu/rpc'
import type {
  MutationOutcome,
  NewProfileRequest_Deserialize,
  ProfileDefinition_Deserialize,
  ProfileId,
  ProfileItem_Serialize,
  ProfileMetadataPatch_Deserialize,
  ProfileSource_Serialize,
  RemoteProfileOptionsPatch_Deserialize,
  TransformKind,
} from '@nyanpasu/rpc/types'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useQueryApi } from '../provider/rpc-provider'
import { invokeMutation, invokeQuery } from './query-options'

// ---- discriminant helpers (successors of the retired NormalizedProfile collapse) ----

export const isConfigItem = (
  item: ProfileItem_Serialize,
): item is Extract<ProfileItem_Serialize, { type: 'config' }> =>
  item.type === 'config'

export const isTransformItem = (
  item: ProfileItem_Serialize,
): item is Extract<ProfileItem_Serialize, { type: 'transform' }> =>
  item.type === 'transform'

/** The source of a File config or a Transform; Composition has none. */
export const getProfileSource = (
  item: ProfileItem_Serialize,
): ProfileSource_Serialize | undefined => {
  if (isTransformItem(item)) return item.transform.source
  return item.config.type === 'file' ? item.config.source : undefined
}

/** Remote = a File config or Transform whose source is a remote URL. */
export const getRemoteSource = (
  item: ProfileItem_Serialize,
): Extract<ProfileSource_Serialize, { type: 'remote' }> | undefined => {
  const source = getProfileSource(item)
  return source?.type === 'remote' ? source : undefined
}

export const isRemoteItem = (item: ProfileItem_Serialize): boolean =>
  getRemoteSource(item) !== undefined

/** Scoped transforms of the current config item (File or Composition). */
export const scopedTransformsOf = (
  item: ProfileItem_Serialize,
): ProfileId[] => {
  if (!isConfigItem(item)) return []
  return item.config.transforms ?? []
}

export type ProfileQueryResult = NonNullable<
  ReturnType<typeof useProfile>['query']['data']
>

export type CreateParams =
  | {
      type: 'url'
      data: {
        url: string
        /** Display name to pin (`custom_name`); `null`/omitted derives it from the URL server-side. */
        name?: string | null
        option?: RemoteProfileOptionsPatch_Deserialize | null
        /** `null`/omitted imports a Config File; otherwise a Transform of this kind. */
        transform?: TransformKind | null
      }
    }
  | {
      type: 'manual'
      data: { request: NewProfileRequest_Deserialize; fileData: string | null }
    }

// Mutations and the `clash_config` state event refetch the profiles, but a
// change that does not touch the runtime (a scheduled sync of an inactive
// subscription) emits nothing. A finite stale time still picks that up on the
// next mount or window focus, without refetching on every route switch.
const PROFILES_STALE_TIME = 30_000

const profilesQuery = (queries: ReturnType<typeof useQueryApi>['queries']) => {
  const profilesOptions = queries.getProfiles()
  return {
    queryKey: profilesOptions.queryKey,
    queryFn: async () => {
      const result = unwrapResult(await invokeQuery(profilesOptions))
      if (!result) return undefined
      return { ...result, items: result.items ?? [] }
    },
    staleTime: PROFILES_STALE_TIME,
  }
}

export const useProfile = (
  options: { refetchInterval?: number | false } = {},
) => {
  const api = useQueryApi()
  const query = useQuery({ ...profilesQuery(api.queries), ...options })

  return { query, ...useProfileMutations() }
}

/** The active config's uid; re-renders only when the active profile changes. */
export const useCurrentProfileUid = () => {
  const api = useQueryApi()
  return useQuery({
    ...profilesQuery(api.queries),
    select: (data) => data?.current,
  }).data
}

/** The profile mutations, for components that do not read the profiles. */
export const useProfileMutations = () => {
  const api = useQueryApi()
  const queryClient = useQueryClient()
  const importProfile = api.mutations.importProfile
  const createProfile = api.mutations.createProfile
  const updateProfile = api.mutations.updateProfile
  const patchProfileMetadata = api.mutations.patchProfileMetadata
  const patchRemoteProfileOptions = api.mutations.patchRemoteProfileOptions
  const replaceProfileDefinition = api.mutations.replaceProfileDefinition
  const activateProfile = api.mutations.activateProfile
  const setProfileValidFields = api.mutations.setProfileValidFields
  const setGlobalTransformsOptions = api.mutations.setGlobalTransforms
  const reorderProfilesByList = api.mutations.reorderProfilesByList
  const deleteProfile = api.mutations.deleteProfile
  const invalidate = () =>
    queryClient.invalidateQueries({
      queryKey: api.queries.getProfiles().queryKey,
    })

  // Profile mutations return the full MutationOutcome so MutationCache can
  // observe `committed_degraded`. Do not collapse to bare values or legacy
  // `{ uid, rebuild }` shapes before React Query onSuccess.
  const create = useMutation({
    mutationKey: createProfile.mutationKey,
    mutationFn: async (
      params: CreateParams,
    ): Promise<MutationOutcome<ProfileId>> => {
      if (params.type === 'url') {
        return unwrapResult(
          await invokeMutation(importProfile, [
            params.data.url,
            params.data.name ?? null,
            params.data.option ?? null,
            params.data.transform ?? null,
          ]),
        )
      }
      return unwrapResult(
        await invokeMutation(createProfile, [
          params.data.request,
          params.data.fileData,
        ]),
      )
    },
    onSuccess: invalidate,
  })

  // Refresh a remote subscription (legacy "update" semantics; the backend
  // returns a domain error for non-remote profiles).
  const update = useMutation({
    mutationKey: updateProfile.mutationKey,
    mutationFn: async ({
      uid,
      option,
    }: {
      uid: ProfileId
      option?: RemoteProfileOptionsPatch_Deserialize | null
    }) =>
      unwrapResult(await invokeMutation(updateProfile, [uid, option ?? null])),
    onSuccess: invalidate,
  })

  const patchMetadata = useMutation({
    mutationKey: patchProfileMetadata.mutationKey,
    mutationFn: async ({
      uid,
      patch,
    }: {
      uid: ProfileId
      patch: ProfileMetadataPatch_Deserialize
    }) =>
      unwrapResult(await invokeMutation(patchProfileMetadata, [uid, patch])),
    onSuccess: invalidate,
  })

  const patchRemoteOptions = useMutation({
    mutationKey: patchRemoteProfileOptions.mutationKey,
    mutationFn: async ({
      uid,
      patch,
    }: {
      uid: ProfileId
      patch: RemoteProfileOptionsPatch_Deserialize
    }) =>
      unwrapResult(
        await invokeMutation(patchRemoteProfileOptions, [uid, patch]),
      ),
    onSuccess: invalidate,
  })

  const replaceDefinition = useMutation({
    mutationKey: replaceProfileDefinition.mutationKey,
    mutationFn: async ({
      uid,
      definition,
    }: {
      uid: ProfileId
      definition: ProfileDefinition_Deserialize
    }) =>
      unwrapResult(
        await invokeMutation(replaceProfileDefinition, [uid, definition]),
      ),
    onSuccess: invalidate,
  })

  const activate = useMutation({
    mutationKey: activateProfile.mutationKey,
    mutationFn: async (uid: ProfileId | null) =>
      unwrapResult(await invokeMutation(activateProfile, [uid])),
    onSuccess: invalidate,
  })

  const setValidFields = useMutation({
    mutationKey: setProfileValidFields.mutationKey,
    mutationFn: async (fields: string[]) =>
      unwrapResult(await invokeMutation(setProfileValidFields, [fields])),
    onSuccess: invalidate,
  })

  const setGlobalTransforms = useMutation({
    mutationKey: setGlobalTransformsOptions.mutationKey,
    mutationFn: async (ids: ProfileId[]) =>
      unwrapResult(await invokeMutation(setGlobalTransformsOptions, [ids])),
    onSuccess: invalidate,
  })

  const sort = useMutation({
    mutationKey: reorderProfilesByList.mutationKey,
    mutationFn: async (uids: ProfileId[]) =>
      unwrapResult(await invokeMutation(reorderProfilesByList, [uids])),
    onSuccess: invalidate,
  })

  const drop = useMutation({
    mutationKey: deleteProfile.mutationKey,
    mutationFn: async (uid: ProfileId) =>
      unwrapResult(await invokeMutation(deleteProfile, [uid])),
    onSuccess: invalidate,
  })

  return {
    create,
    update,
    patchMetadata,
    patchRemoteOptions,
    replaceDefinition,
    activate,
    setValidFields,
    setGlobalTransforms,
    sort,
    drop,
  }
}
