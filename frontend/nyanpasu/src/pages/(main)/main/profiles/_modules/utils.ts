import { isConfigItem, isTransformItem } from '@nyanpasu/query'
import {
  type ProfileItem_Serialize,
  type ProfileListingItem_Serialize,
} from '@nyanpasu/rpc/types'
import { ProfileType } from './consts'

export type ProfileSummaryLike =
  ProfileItem_Serialize | ProfileListingItem_Serialize

export type ConfigProfile<
  T extends ProfileSummaryLike = ProfileItem_Serialize,
> = Extract<T, { type: 'config' }>
export type TransformProfile<
  T extends ProfileSummaryLike = ProfileItem_Serialize,
> = Extract<T, { type: 'transform' }>

export const transformLabelOf = (profile: ProfileSummaryLike) => {
  if (profile.type !== 'transform') return undefined
  if (profile.transform.type === 'overlay') return 'YAML'

  return profile.transform.runtime === 'javascript' ? 'JavaScript' : 'Lua'
}

export type CategoryProfiles = {
  [ProfileType.Profile]: ConfigProfile[]
  [ProfileType.JavaScript]: TransformProfile[]
  [ProfileType.Lua]: TransformProfile[]
  [ProfileType.Merge]: TransformProfile[]
}

/** Activatable = Config definition (File / Composition). */
export const isProxyProfile = <T extends ProfileSummaryLike>(
  profile: T,
): profile is ConfigProfile<T> => isConfigItem(profile)

/** Chain candidate = Transform definition (Overlay / Script). */
export const isChainProfile = <T extends ProfileSummaryLike>(
  profile: T,
): profile is TransformProfile<T> => isTransformItem(profile)

export const isJavaScriptProfile = <T extends ProfileSummaryLike>(
  profile: T,
): profile is TransformProfile<T> =>
  isTransformItem(profile) &&
  profile.transform.type === 'script' &&
  profile.transform.runtime === 'javascript'

export const isLuaProfile = <T extends ProfileSummaryLike>(
  profile: T,
): profile is TransformProfile<T> =>
  isTransformItem(profile) &&
  profile.transform.type === 'script' &&
  profile.transform.runtime === 'lua'

export const isMergeProfile = <T extends ProfileSummaryLike>(
  profile: T,
): profile is TransformProfile<T> =>
  isTransformItem(profile) && profile.transform.type === 'overlay'

export const categoryProfiles = (
  profiles?: ProfileItem_Serialize[] | null,
): CategoryProfiles => {
  const categorized: CategoryProfiles = {
    [ProfileType.Profile]: [],
    [ProfileType.JavaScript]: [],
    [ProfileType.Lua]: [],
    [ProfileType.Merge]: [],
  }

  for (const profile of profiles ?? []) {
    if (isProxyProfile(profile)) {
      categorized[ProfileType.Profile].push(profile)
    } else if (isJavaScriptProfile(profile)) {
      categorized[ProfileType.JavaScript].push(profile)
    } else if (isLuaProfile(profile)) {
      categorized[ProfileType.Lua].push(profile)
    } else if (isMergeProfile(profile)) {
      categorized[ProfileType.Merge].push(profile)
    }
  }

  return categorized
}

/** The `$type` route segment a profile's detail page lives under. */
export const profileTypeOf = (profile: ProfileSummaryLike): ProfileType => {
  if (isJavaScriptProfile(profile)) return ProfileType.JavaScript
  if (isLuaProfile(profile)) return ProfileType.Lua
  if (isMergeProfile(profile)) return ProfileType.Merge
  return ProfileType.Profile
}

/**
 * Whether the profile owns a file on disk. Composition configs have none, so
 * viewing or opening them would hit `ProfileHasNoFile`; file configs and
 * transforms both own one.
 */
export const hasMaterializedFile = (profile: ProfileSummaryLike) =>
  isTransformItem(profile) ||
  (profile.type === 'config' && profile.config.type === 'file')
