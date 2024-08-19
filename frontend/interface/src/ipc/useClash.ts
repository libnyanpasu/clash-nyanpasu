import useSWR from "swr";
import * as tauri from "@/service/tauri";
import { ClashConfig, Profile } from "../../dist";
import { Clash, clash } from "../service/clash";

/**
 * useClash with swr.
 * Data from tauri backend.
 */
export const useClash = () => {
  const { deleteConnections, ...api } = clash();

  const getClashInfo = useSWR("getClashInfo", tauri.getClashInfo);

  const setClashInfo = async (payload: Partial<ClashConfig>) => {
    try {
      await tauri.patchClashInfo(payload);

      await getClashInfo.mutate();
    } catch (e) {
      console.error(e);
    } finally {
      return getClashInfo.data;
    }
  };

  const getConfigs = useSWR("getClashConfig", api.getConfigs);

  const setConfigs = async (payload: Partial<Clash.Config>) => {
    try {
      await api.setConfigs(payload);

      await getConfigs.mutate();
    } finally {
      return getConfigs.data;
    }
  };

  const getVersion = useSWR("getClashVersion", api.getVersion);

  const getRules = useSWR("getClashRules", api.getRules);

  const getRuntimeExists = useSWR("getRuntimeExists", tauri.getRuntimeExists);

  const getProfiles = useSWR("getProfiles", tauri.getProfiles);

  const setProfiles = async (index: string, profile: Partial<Profile.Item>) => {
    await tauri.setProfiles({ index, profile });

    await getProfiles.mutate();

    await getRuntimeLogs.mutate();
  };

  const setProfilesConfig = async (profiles: Profile.Config) => {
    await tauri.setProfilesConfig(profiles);

    await getProfiles.mutate();

    await getRuntimeLogs.mutate();
  };

  const createProfile = async (item: Partial<Profile.Item>, data?: string) => {
    await tauri.createProfile(item, data);

    await getProfiles.mutate();
  };

  const updateProfile = async (uid: string, option?: Profile.Option) => {
    await tauri.updateProfile(uid, option);

    await getProfiles.mutate();
  };

  const deleteProfile = async (uid: string) => {
    await tauri.deleteProfile(uid);

    await getProfiles.mutate();
  };

  const getProfileFile = async (id?: string) => {
    if (id) {
      const result = await tauri.readProfileFile(id);

      if (result) {
        return result;
      } else {
        return "";
      }
    } else {
      return "";
    }
  };

  const importProfile = async (url: string, option?: Profile.Option) => {
    await tauri.importProfile(url, option);

    await getProfiles.mutate();
  };

  const getRuntimeLogs = useSWR("getRuntimeLogs", tauri.getRuntimeLogs, {
    refreshInterval: 1000,
  });

  return {
    getClashInfo,
    setClashInfo,
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
  };
};
