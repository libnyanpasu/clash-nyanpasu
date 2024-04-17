import { invoke } from "@tauri-apps/api/tauri";
import { ClashInfo, VergeConfig } from "./types";

export const getNyanpasuConfig = async () => {
  return await invoke<VergeConfig>("get_verge_config");
};

export const patchNyanpasuConfig = async (payload: VergeConfig) => {
  return await invoke<void>("patch_verge_config", { payload });
};

export const getClashInfo = async () => {
  return await invoke<ClashInfo | null>("get_clash_info");
};
