import { useAtomValue, useSetAtom } from "jotai";
import { useEffect, useState } from "react";
import { OS } from "@/consts";
import { UpdaterIgnoredAtom, UpdaterManifestAtom } from "@/store/updater";
import { useNyanpasu } from "@nyanpasu/interface";
import { checkUpdate } from "@tauri-apps/api/updater";
import { useIsAppImage } from "./use-consts";

export function useUpdaterPlatformSupported() {
  const [supported, setSupported] = useState(false);
  const isAppImage = useIsAppImage();
  useEffect(() => {
    switch (OS) {
      case "macos":
      case "windows":
        setSupported(true);
        break;
      case "linux":
        setSupported(!!isAppImage.data);
        break;
    }
  }, [isAppImage.data]);
  return supported;
}

export default function useUpdater() {
  const { nyanpasuConfig } = useNyanpasu();
  const updaterIgnored = useAtomValue(UpdaterIgnoredAtom);
  const setUpdaterManifest = useSetAtom(UpdaterManifestAtom);
  const isPlatformSupported = useUpdaterPlatformSupported();

  useEffect(() => {
    const run = async () => {
      if (nyanpasuConfig?.enable_auto_check_update && isPlatformSupported) {
        const info = await checkUpdate();
        if (info?.shouldUpdate && updaterIgnored !== info.manifest?.version) {
          setUpdaterManifest(info.manifest || null);
        }
      }
    };
    run().catch(console.error);
  }, [
    isPlatformSupported,
    nyanpasuConfig?.enable_auto_check_update,
    setUpdaterManifest,
    updaterIgnored,
  ]);
}
