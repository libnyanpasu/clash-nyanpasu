import { useAtom } from "jotai";
import { lazy, Suspense, useState } from "react";
import { UpdaterManifestAtom } from "@/store/updater";

const UpdaterDialog = lazy(() => import("./updater-dialog"));

export const UpdaterDialogWrapper = () => {
  const [open, setOpen] = useState(true);
  const [manifest, setManifest] = useAtom(UpdaterManifestAtom);
  if (!manifest) return null;
  return (
    <Suspense fallback={null}>
      <UpdaterDialog
        open={open}
        onClose={() => {
          setOpen(false);
          setManifest(null);
        }}
        manifest={manifest}
      />
    </Suspense>
  );
};

export default UpdaterDialogWrapper;
