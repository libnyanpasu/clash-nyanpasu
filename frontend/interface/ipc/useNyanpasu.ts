import useSWR from "swr";
import * as service from "@/service";
import { VergeConfig } from "@/service";
import { fetchCoreVersion, fetchLatestCore } from "@/service/core";
import { useClash } from "./useClash";
import { useMemo } from "react";
import * as tauri from "@/service/tauri";

/**
 * useNyanpasu with swr.
 * Data from tauri backend.
 */
export const useNyanpasu = (options?: {
  onUpdate?: (data?: VergeConfig) => void;
  onError?: (error: any) => void;
}) => {
  const { getConfigs, setConfigs, deleteConnections } = useClash();

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

  const getLatestCore = useSWR("getLatestCore", fetchLatestCore, {
    revalidateOnMount: false,
  });

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

  const setCurrentMode = async (mode: string) => {
    await deleteConnections();

    await setConfigs({ mode });

    await mutate();
  };

  const getCurrentMode = useMemo(() => {
    const modes: { [key: string]: boolean } = {
      rule: false,
      global: false,
      direct: false,
    };

    if (data?.clash_core == "clash") {
      modes.script = false;
    }

    const mode = getConfigs.data?.mode?.toLowerCase();

    if (mode && modes.hasOwnProperty(mode)) {
      modes[mode] = true;
    } else {
      modes.rule = true;
    }

    return modes;
  }, [data?.clash_core, getConfigs.data?.mode]);

  return {
    nyanpasuConfig: data,
    isLoading: !data && !error,
    isError: error,
    setNyanpasuConfig,
    getCoreVersion: tauri.getCoreVersion,
    getClashCore,
    setClashCore,
    restartSidecar: tauri.restartSidecar,
    getLatestCore,
    updateCore,
    getSystemProxy,
    getServiceStatus,
    setServiceStatus,
    getCurrentMode,
    setCurrentMode,
  };
};
