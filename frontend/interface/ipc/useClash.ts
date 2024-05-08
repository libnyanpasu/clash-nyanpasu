import useSWR from "swr";
import { Clash, clash } from "../service/clash";
import * as tauri from "@/service/tauri";
import { ClashConfig, Profile } from "..";

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

  const setProfiles = async (payload: {
    index: string;
    profile: Partial<Profile.Item>;
  }) => {
    try {
      await tauri.setProfiles(payload);

      await getProfiles.mutate();
    } finally {
      return getProfiles.data;
    }
  };

  const setProfilesConfig = async (profiles: Profile.Config) => {
    try {
      await tauri.setProfilesConfig(profiles);

      await getProfiles.mutate();
    } finally {
      return getProfiles.data;
    }
  };

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
  };
};
