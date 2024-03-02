import { useVerge } from "@/hooks/use-verge";
import { Button } from "@mui/material";
import { checkUpdate } from "@tauri-apps/api/updater";
import { useRef } from "react";
import useSWR from "swr";
import { DialogRef } from "../base";
import { UpdateViewer } from "../setting/mods/update-viewer";

interface Props {
  className?: string;
}

export const UpdateButton = (props: Props) => {
  const { className } = props;

  const viewerRef = useRef<DialogRef>(null);

  const { verge } = useVerge();

  const { disable_auto_check_update } = verge ?? {};

  const { data: updateInfo } = useSWR(
    disable_auto_check_update ? null : "checkUpdate",
    disable_auto_check_update ? null : checkUpdate,
    {
      errorRetryCount: 2,
      revalidateIfStale: false,
      focusThrottleInterval: 36e5, // 1 hour
    },
  );

  if (!updateInfo?.shouldUpdate) return null;

  return (
    <>
      <UpdateViewer ref={viewerRef} />

      <Button
        color="error"
        variant="contained"
        size="small"
        className={className}
        onClick={() => viewerRef.current?.open()}
      >
        New
      </Button>
    </>
  );
};
