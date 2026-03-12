import { findKey } from 'lodash-es'
import { Profile } from '@nyanpasu/interface'
import { PROFILE_TYPES, ProfileType } from '../../_modules/consts'

export type CategoryProfiles = {
  [ProfileType.Profile]: Array<Extract<Profile, { type: 'local' | 'remote' }>>
  [ProfileType.JavaScript]: Array<
    Extract<Profile, { type: 'script'; script_type: 'javascript' }>
  >
  [ProfileType.Lua]: Array<
    Extract<Profile, { type: 'script'; script_type: 'lua' }>
  >
  [ProfileType.Merge]: Array<Extract<Profile, { type: 'merge' }>>
}

export const isProxyProfile = (
  profile: Profile,
): profile is CategoryProfiles[ProfileType.Profile][number] =>
  profile.type === 'local' || profile.type === 'remote'

export const isJavaScriptProfile = (
  profile: Profile,
): profile is CategoryProfiles[ProfileType.JavaScript][number] =>
  profile.type === 'script' && profile.script_type === 'javascript'

export const isLuaProfile = (
  profile: Profile,
): profile is CategoryProfiles[ProfileType.Lua][number] =>
  profile.type === 'script' && profile.script_type === 'lua'

export const isMergeProfile = (
  profile: Profile,
): profile is CategoryProfiles[ProfileType.Merge][number] =>
  profile.type === 'merge'

export const categoryProfiles = (profiles: Profile[]): CategoryProfiles => {
  const initialCategorized: CategoryProfiles = {
    [ProfileType.Profile]: [],
    [ProfileType.JavaScript]: [],
    [ProfileType.Lua]: [],
    [ProfileType.Merge]: [],
  }

  return profiles.reduce((categorized, profile) => {
    const matchedProfileType = findKey(PROFILE_TYPES, (allowedTypes) =>
      allowedTypes.some((allowedType) => {
        if (allowedType.type !== profile.type) {
          return false
        }

        if (
          'script_type' in allowedType &&
          allowedType.script_type !== undefined
        ) {
          return (
            profile.type === 'script' &&
            profile.script_type === allowedType.script_type
          )
        }

        return true
      }),
    ) as ProfileType | undefined

    if (!matchedProfileType) {
      return categorized
    }

    switch (matchedProfileType) {
      case ProfileType.Profile:
        if (isProxyProfile(profile)) {
          categorized[ProfileType.Profile].push(profile)
        }
        break

      case ProfileType.JavaScript:
        if (isJavaScriptProfile(profile)) {
          categorized[ProfileType.JavaScript].push(profile)
        }
        break

      case ProfileType.Lua:
        if (isLuaProfile(profile)) {
          categorized[ProfileType.Lua].push(profile)
        }
        break

      case ProfileType.Merge:
        if (isMergeProfile(profile)) {
          categorized[ProfileType.Merge].push(profile)
        }
        break
    }

    return categorized
  }, initialCategorized)
}
