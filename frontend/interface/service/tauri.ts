import { invoke } from "@tauri-apps/api/tauri";
import {
  ClashConfig,
  ClashInfo,
  VergeConfig,
  Profile,
  SystemProxy,
  Proxies,
} from "./types";
import { ManifestVersion } from "./core";

export const getNyanpasuConfig = async () => {
  return await invoke<VergeConfig>("get_verge_config");
};

export const patchNyanpasuConfig = async (payload: VergeConfig) => {
  return await invoke<void>("patch_verge_config", { payload });
};

export const getClashInfo = async () => {
  return await invoke<ClashInfo | null>("get_clash_info");
};

export const patchClashInfo = async (payload: Partial<ClashConfig>) => {
  return await invoke<void>("patch_clash_config", { payload });
};

export const getRuntimeExists = async () => {
  return await invoke<string[]>("get_runtime_exists");
};

export const getRuntimeLogs = async () => {
  return await invoke<Record<string, [string, string][]>>("get_runtime_logs");
};

export const createProfile = async (
  item: Partial<Profile.Item>,
  fileData?: string | null,
) => {
  return await invoke<void>("create_profile", { item, fileData });
};

export const updateProfile = async (uid: string, option?: Profile.Option) => {
  return await invoke<void>("update_profile", { index: uid, option });
};

export const deleteProfile = async (uid: string) => {
  return await invoke<void>("delete_profile", { index: uid });
};

export const viewProfile = async (uid: string) => {
  return await invoke<void>("view_profile", { index: uid });
};

export const getProfiles = async () => {
  return await invoke<Profile.Config>("get_profiles");
};

export const setProfiles = async (payload: {
  index: string;
  profile: Partial<Profile.Item>;
}) => {
  return await invoke<void>("patch_profile", payload);
};

export const setProfilesConfig = async (profiles: Profile.Config) => {
  return await invoke<void>("patch_profiles_config", { profiles });
};

export const readProfileFile = async (index: string) => {
  return await invoke<string>("read_profile_file", { index });
};

export const saveProfileFile = async (index: string, fileData: string) => {
  return await invoke<void>("save_profile_file", { index, fileData });
};

export const importProfile = async (
  url: string,
  option: Profile.Option = { with_proxy: true },
) => {
  return await invoke<void>("import_profile", {
    url,
    option,
  });
};

export const getCoreVersion = async (
  coreType: Required<VergeConfig>["clash_core"],
) => {
  return await invoke<string>("get_core_version", { coreType });
};

export const setClashCore = async (
  clashCore: Required<VergeConfig>["clash_core"],
) => {
  return await invoke<void>("change_clash_core", { clashCore });
};

export const restartSidecar = async () => {
  return await invoke<void>("restart_sidecar");
};

export const fetchLatestCoreVersions = async () => {
  return await invoke<ManifestVersion["latest"]>("fetch_latest_core_versions");
};

export const updateCore = async (
  coreType: Required<VergeConfig>["clash_core"],
) => {
  return await invoke<void>("update_core", { coreType });
};

export const pullupUWPTool = async () => {
  return await invoke<void>("invoke_uwp_tool");
};

export const getSystemProxy = async () => {
  return await invoke<SystemProxy>("get_sys_proxy");
};

export const checkService = async () => {
  try {
    const result = await invoke<{ code: number }>("check_service");

    if (result?.code === 0) {
      return "active";
    } else if (result?.code === 400) {
      return "installed";
    } else {
      return "unknown";
    }
  } catch (e) {
    return "uninstall";
  }
};

export const installService = async () => {
  return await invoke<void>("install_service");
};

export const uninstallService = async () => {
  return await invoke<void>("uninstall_service");
};

export const openAppDir = async () => {
  return await invoke<void>("open_app_dir");
};

export const openCoreDir = async () => {
  return await invoke<void>("open_core_dir");
};

export const openLogsDir = async () => {
  return await invoke<void>("open_logs_dir");
};

export const collectLogs = async () => {
  return await invoke<void>("collect_logs");
};

export const setCustomAppDir = async (path: string) => {
  return await invoke<void>("set_custom_app_dir", { path });
};

export const restartApplication = async () => {
  return await invoke<void>("restart_application");
};

export const isPortable = async () => {
  return await invoke<boolean>("is_portable");
};

export const getProxies = async () => {
  return await invoke<Proxies>("get_proxies");
};

export const selectProxy = async (group: string, name: string) => {
  return await invoke<void>("select_proxy", { group, name });
};

export const updateProxyProvider = async (name: string) => {
  return await invoke<void>("update_proxy_provider", { name });
};

export const save_window_size_state = async () => {
  return await invoke<void>("save_window_size_state");
};
