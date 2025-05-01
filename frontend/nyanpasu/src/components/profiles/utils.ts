import { isEqual } from 'lodash-es'
import type {
  LocalProfile,
  MergeProfile,
  Profile,
  RemoteProfile,
  ScriptProfile,
} from '@nyanpasu/interface'

/**
 * Represents a Clash configuration profile, which can be either locally stored or fetched from a remote source.
 */
export type ClashProfile = LocalProfile | RemoteProfile

/**
 * Represents a Clash configuration profile that is a chain of multiple profiles.
 */
export type ChainProfile = MergeProfile | ScriptProfile

/**
 * Filters an array of profiles into two categories: clash and chain profiles.
 *
 * @param items - Array of Profile objects to be filtered
 * @returns An object containing two arrays:
 *          - clash: Array of profiles where type is 'remote' or 'local'
 *          - chain: Array of profiles where type is 'merge' or has a script property
 */
export function filterProfiles<T extends Profile>(items?: T[]) {
  /**
   * Filters the input array to include only items of type 'remote' or 'local'
   * @param items - Array of items to filter
   * @returns {Array} Filtered array containing only remote and local items
   */
  const clash = items?.filter(
    (item) => item.type === 'remote' || item.type === 'local',
  )

  /**
   * Filters an array of items to get a chain of either 'merge' type items
   * or items with a script property in their type object.
   *
   * @param {Array<{ type: string | { script: 'javascript' | 'lua' } }>} items - The array of items to filter
   * @returns {Array<{ type: string | { script: 'javascript' | 'lua' } }>} A filtered array containing only merge items or items with scripts
   */
  const chain = items?.filter(
    (item) =>
      item.type === 'merge' ||
      (typeof item.type === 'object' && item.type.script),
  )

  return {
    clash,
    chain,
  }
}

export type ProfileType = Profile['type']

export const ProfileTypes = {
  JavaScript: { script: 'javascript' },
  LuaScript: { script: 'lua' },
  Merge: 'merge',
} as const

export const getLanguage = (type: ProfileType, snake?: boolean) => {
  switch (true) {
    case isEqual(type, ProfileTypes.JavaScript):
    case isEqual(type, ProfileTypes.JavaScript.script): {
      return snake ? 'JavaScript' : 'javascript'
    }

    case isEqual(type, ProfileTypes.LuaScript):
    case isEqual(type, ProfileTypes.LuaScript.script): {
      return snake ? 'Lua' : 'lua'
    }

    case isEqual(type, ProfileTypes.Merge): {
      return snake ? 'YAML' : 'yaml'
    }
  }
}
