import { invoke } from "@tauri-apps/api/tauri";
import { ClashConfig, ClashInfo, VergeConfig, Profile } from "./types";

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
