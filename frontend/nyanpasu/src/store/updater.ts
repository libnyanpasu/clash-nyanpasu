import { atom } from "jotai";
import { atomWithStorage } from "jotai/utils";
import { UpdateManifest } from "@tauri-apps/api/updater";

export const UpdaterIgnoredAtom = atomWithStorage(
  "updaterIgnored",
  null as string | null,
);

export const UpdaterManifestAtom = atom<UpdateManifest | null>(null);
