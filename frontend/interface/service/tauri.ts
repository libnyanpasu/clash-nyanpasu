import { invoke } from "@tauri-apps/api/tauri";
import { VergeConfig } from "./types";

export const nyanpasuConfig = {
  get: async () => {
    return await invoke<VergeConfig>("get_verge_config");
  },

  set: async (payload: VergeConfig) => {
    return await invoke<void>("patch_verge_config", { payload });
  },
};
