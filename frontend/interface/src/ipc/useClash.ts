import useSWR from 'swr'
import {
  ClashConfig,
  Profile,
  ProfilesBuilder,
  RemoteProfileOptionsBuilder,
} from '@/index'
import * as tauri from '@/service/tauri'
import { clash } from '../service/clash'

/**
 * useClash with swr.
 * Data from tauri backend.
 */
export const useClash = () => {
  const { deleteConnections, ...api } = clash()

  const getClashInfo = useSWR('getClashInfo', tauri.getClashInfo)
  const getConfigs = useSWR('getClashConfig', api.getConfigs)

  const setConfigs = async (payload: Partial<ClashConfig>) => {
    try {
      await tauri.patchClashConfig(payload)

      await Promise.all([getClashInfo.mutate(), getConfigs.mutate()])
    } catch (e) {
      console.error(e)
    }

    return getClashInfo.data
  }

  const getVersion = useSWR('getClashVersion', api.getVersion)

  const getRules = useSWR('getClashRules', api.getRules)

  const getRuntimeExists = useSWR('getRuntimeExists', tauri.getRuntimeExists)

  const getProfiles = useSWR('getProfiles', tauri.getProfiles)

  const setProfiles = async (uid: string, profile: Partial<Profile>) => {
    await tauri.setProfiles({ uid, profile })

    await getProfiles.mutate()

    await getRuntimeLogs.mutate()
  }

  const setProfilesConfig = async (profiles: ProfilesBuilder) => {
    await tauri.setProfilesConfig(profiles)

    await getProfiles.mutate()

    await getRuntimeLogs.mutate()
  }

  const createProfile = async (item: Partial<Profile>, data?: string) => {
    await tauri.createProfile(item, data)

    await getProfiles.mutate()
  }

  const updateProfile = async (
    uid: string,
    option?: RemoteProfileOptionsBuilder,
  ) => {
    await tauri.updateProfile(uid, option)

    await getProfiles.mutate()
  }

  const deleteProfile = async (uid: string) => {
    await tauri.deleteProfile(uid)

    await getProfiles.mutate()
  }

  const getProfileFile = async (id?: string) => {
    if (id) {
      const result = await tauri.readProfileFile(id)

      if (result) {
        return result
      } else {
        return ''
      }
    } else {
      return ''
    }
  }

  const importProfile = async (
    url: string,
    option: RemoteProfileOptionsBuilder,
  ) => {
    await tauri.importProfile(url, option)

    await getProfiles.mutate()
  }

  const getRuntimeLogs = useSWR('getRuntimeLogs', tauri.getRuntimeLogs, {
    refreshInterval: 1000,
  })

  const reorderProfilesByList = async (list: string[]) => {
    await tauri.reorderProfilesByList(list)

    await getProfiles.mutate()
  }

  return {
    getClashInfo,
    getConfigs,
    setConfigs,
    getVersion,
    getRules,
    deleteConnections,
    getRuntimeExists,
    getProfiles,
    setProfiles,
    setProfilesConfig,
    createProfile,
    updateProfile,
    deleteProfile,
    importProfile,
    viewProfile: tauri.viewProfile,
    getProfileFile,
    getRuntimeLogs,
    setProfileFile: tauri.saveProfileFile,
    reorderProfilesByList,
  }
}
