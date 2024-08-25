import { useAtomValue, useSetAtom } from "jotai";
import { useMount } from "react-use";
import { UpdaterIgnoredAtom, UpdaterManifestAtom } from "@/store/updater";
import { useNyanpasu } from "@nyanpasu/interface";
import { checkUpdate } from "@tauri-apps/api/updater";

export default function useUpdater() {
  const { nyanpasuConfig } = useNyanpasu();
  const updaterIgnored = useAtomValue(UpdaterIgnoredAtom);
  const setUpdaterManifest = useSetAtom(UpdaterManifestAtom);

  useMount(() => {
    const run = async () => {
      if (nyanpasuConfig?.enable_auto_check_update) {
        const info = await checkUpdate();
        if (info?.shouldUpdate && updaterIgnored !== info.manifest?.version) {
          setUpdaterManifest(info.manifest || null);
        }
      }
    };
    run().catch(console.error);
  });
}
