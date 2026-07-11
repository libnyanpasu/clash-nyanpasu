import {
  isConfigItem,
  isTransformItem,
  type ProfileItem_Serialize,
} from '@nyanpasu/interface'
import { ProfileType } from '../../_modules/consts'

export type ConfigProfile = Extract<ProfileItem_Serialize, { type: 'config' }>
export type TransformProfile = Extract<
  ProfileItem_Serialize,
  { type: 'transform' }
>

export type CategoryProfiles = {
  [ProfileType.Profile]: ConfigProfile[]
  [ProfileType.JavaScript]: TransformProfile[]
  [ProfileType.Lua]: TransformProfile[]
  [ProfileType.Merge]: TransformProfile[]
}

/** Activatable = Config definition (File / Composition). */
export const isProxyProfile = (
  profile: ProfileItem_Serialize,
): profile is ConfigProfile => isConfigItem(profile)

/** Chain candidate = Transform definition (Overlay / Script). */
export const isChainProfile = (
  profile: ProfileItem_Serialize,
): profile is TransformProfile => isTransformItem(profile)

export const isJavaScriptProfile = (
  profile: ProfileItem_Serialize,
): profile is TransformProfile =>
  isTransformItem(profile) &&
  profile.transform.type === 'script' &&
  profile.transform.runtime === 'javascript'

export const isLuaProfile = (
  profile: ProfileItem_Serialize,
): profile is TransformProfile =>
  isTransformItem(profile) &&
  profile.transform.type === 'script' &&
  profile.transform.runtime === 'lua'

export const isMergeProfile = (
  profile: ProfileItem_Serialize,
): profile is TransformProfile =>
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
