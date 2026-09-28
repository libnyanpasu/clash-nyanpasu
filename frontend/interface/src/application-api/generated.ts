/* Generated from backend/tauri/gen/application-api.json. Do not edit. */
import type { ActivateProfileInput, ActivateProfileResult, ClashWsEvent, ClashWsSnapshot, EmptyInput, ProfilesList } from "./types";
export type * from "./types";

export type ApplicationApiUnary = {
  "clash.snapshot": { params: EmptyInput; result: ClashWsSnapshot }
  "profiles.activate": { params: ActivateProfileInput; result: ActivateProfileResult }
  "profiles.list": { params: EmptyInput; result: ProfilesList }
}

export type ApplicationApiStreams = {
  "clash.events": { params: null; event: ClashWsEvent }
}

export const applicationApiProcedureNames = ["clash.events","clash.snapshot","profiles.activate","profiles.list"] as const
