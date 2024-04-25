import useSWR from "swr";
import * as service from "@/service";
import { VergeConfig, restartSidecar, getCoreVersion } from "@/service";
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
    service.getNyanpasuConfig,
  );

  const setNyanpasuConfig = async (payload: Partial<VergeConfig>) => {
    try {
      await service.patchNyanpasuConfig(payload);

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
    await service.setClashCore(clashCore);

    // timeout for restart clash core.
    setTimeout(() => {
      getClashCore.mutate();
    }, 100);
  };

  const getLatestCore = useSWR("getLatestCore", fetchLatestCore);

  const updateCore = async (core: Required<VergeConfig>["clash_core"]) => {
    await service.updateCore(core);

    getClashCore.mutate();
  };

  const getSystemProxy = useSWR("getSystemProxy", service.getSystemProxy);

  const getServiceStatus = useSWR("getServiceStatus", service.checkService);

  const setServiceStatus = async (type: "install" | "uninstall") => {
    if (type === "install") {
      await service.installService();
    } else {
      await service.uninstallService();
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
