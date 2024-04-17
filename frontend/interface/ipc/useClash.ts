import useSWR from "swr";
import { clash } from "../service/clash";

/**
 * useClash with swr.
 * Data from tauri backend.
 */
export const useClash = () => {
  const { setConfigs, setProxies, deleteConnections, ...api } = clash();

  const getConfigs = useSWR("getClashConfig", api.getConfigs);

  const getVersion = useSWR("getClashVersion", api.getVersion);

  const getRules = useSWR("getClashRules", api.getRules);

  const getProxiesDelay = useSWR("getClashProxiesDelay", api.getProxiesDelay);

  const getProxies = useSWR("getClashProxies", api.getProxies);

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
