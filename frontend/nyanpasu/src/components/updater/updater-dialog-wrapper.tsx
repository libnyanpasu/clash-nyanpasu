import { UpdaterInstanceAtom } from "@/store/updater";
import { useAtom } from "jotai";
import { lazy, Suspense, useState } from "react";

const UpdaterDialog = lazy(() => import("./updater-dialog"));

export const UpdaterDialogWrapper = () => {
  const [open, setOpen] = useState(true);
  const [manifest, setManifest] = useAtom(UpdaterInstanceAtom);
  if (!manifest) return null;
  return (
    <Suspense fallback={null}>
      <UpdaterDialog
        open={open}
        onClose={() => {
          setOpen(false);
          setManifest(null);
        }}
        update={manifest}
      />
    </Suspense>
  );
};

export default UpdaterDialogWrapper;
