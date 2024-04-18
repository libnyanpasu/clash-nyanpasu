import useSWR from "swr";
import { Clash, clash } from "../service/clash";

/**
 * useClash with swr.
 * Data from tauri backend.
 */
export const useClash = () => {
  const { deleteConnections, ...api } = clash();

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

  const getProxiesDelay = useSWR("getClashProxiesDelay", api.getProxiesDelay);

  const getProxies = useSWR("getClashProxies", api.getProxies);

  const setProxies = async (payload: { group: string; proxy: string }) => {
    try {
      await api.setProxies(payload);

      await getProxies.mutate();
    } finally {
      return getProxies.data;
    }
  };

  return {
    getConfigs,
    setConfigs,
    getVersion,
    getRules,
    getProxiesDelay,
    getProxies,
    setProxies,
    deleteConnections,
  };
};
