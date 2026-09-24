import { invoke } from "@tauri-apps/api/core";

import { configureApiDispatch, type ApiResponse } from "./api-transport";

configureApiDispatch((request) =>
  invoke<ApiResponse>("api_dispatch", { request })
);
