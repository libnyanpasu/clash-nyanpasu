import useSWR from "swr";
import {
  getNyanpasuConfig,
  patchNyanpasuConfig,
  VergeConfig,
  getCoreVersion,
  setClashCore as setClashCoreWithTauri,
  restartSidecar,
  updateCore as updateCoreWithTauri,
  getSystemProxy as getSystemProxyWithTauri,
  checkService,
  installService,
  uninstallService,
} from "@/service";
import { fetchCoreVersion, fetchLatestCore } from "@/service/core";

/**
 * useNyanpasu with swr.
 * Data from tauri backend.
 */
export const useNyanpasu = (options?: {
  onUpdate?: (data?: VergeConfig) => void;
  onError?: (error: any) => void;
}) => {
  const { data, error, mutate } = useSWR<VergeConfig>(
    "nyanpasuConfig",
    getNyanpasuConfig,
  );

  const setNyanpasuConfig = async (payload: Partial<VergeConfig>) => {
    try {
      await patchNyanpasuConfig(payload);

      const result = await mutate();

      if (options?.onUpdate) {
        options?.onUpdate(result);
      }
    } catch (error) {
      if (options?.onError) {
        options?.onError(error);
      }
    }
  };

  const getClashCore = useSWR("getClashCore", fetchCoreVersion);

  const setClashCore = async (
    clashCore: Required<VergeConfig>["clash_core"],
  ) => {
    await setClashCoreWithTauri(clashCore);

    // timeout for restart clash core.
    setTimeout(() => {
      getClashCore.mutate();
    }, 100);
  };

  const getLatestCore = useSWR("getLatestCore", fetchLatestCore);

  const updateCore = async (core: Required<VergeConfig>["clash_core"]) => {
    await updateCoreWithTauri(core);

    getClashCore.mutate();
  };

  const getSystemProxy = useSWR("getSystemProxy", getSystemProxyWithTauri);

  const getServiceStatus = useSWR("getServiceStatus", checkService);

  const setServiceStatus = async (type: "install" | "uninstall") => {
    if (type === "install") {
      await installService();
    } else {
      await uninstallService();
    }

    return getServiceStatus.mutate();
  };

  return {
    nyanpasuConfig: data,
    isLoading: !data && !error,
    isError: error,
    setNyanpasuConfig,
    getCoreVersion,
    getClashCore,
    setClashCore,
    restartSidecar,
    getLatestCore,
    updateCore,
    getSystemProxy,
    getServiceStatus,
    setServiceStatus,
  };
};
