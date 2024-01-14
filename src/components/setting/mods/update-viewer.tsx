import { BaseDialog, DialogRef } from "@/components/base";
import { NotificationType, useNotification } from "@/hooks/use-notification";
import { atomUpdateState } from "@/services/states";
import { Box, styled } from "@mui/material";
import { relaunch } from "@tauri-apps/api/process";
import { checkUpdate, installUpdate } from "@tauri-apps/api/updater";
import { useLockFn } from "ahooks";
import { forwardRef, useImperativeHandle, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useRecoilState } from "recoil";
import snarkdown from "snarkdown";
import useSWR from "swr";

const UpdateLog = styled(Box)(() => ({
  "h1,h2,h3,ul,ol,p": { margin: "0.5em 0", color: "inherit" },
}));

export const UpdateViewer = forwardRef<DialogRef>((props, ref) => {
  const { t } = useTranslation();

  const [open, setOpen] = useState(false);
  const [updateState, setUpdateState] = useRecoilState(atomUpdateState);

  const { data: updateInfo } = useSWR("checkUpdate", checkUpdate, {
    errorRetryCount: 2,
    revalidateIfStale: false,
    focusThrottleInterval: 36e5, // 1 hour
  });

  useImperativeHandle(ref, () => ({
    open: () => setOpen(true),
    close: () => setOpen(false),
  }));

  // markdown parser
  const parseContent = useMemo(() => {
    if (!updateInfo?.manifest?.body) {
      return "New Version is available";
    }
    return snarkdown(updateInfo?.manifest?.body);
  }, [updateInfo]);

  const onUpdate = useLockFn(async () => {
    if (updateState) return;
    setUpdateState(true);

    try {
      await installUpdate();
      await relaunch();
    } catch (err: any) {
      useNotification({
        title: t("Error"),
        body: err.message || err.toString(),
        type: NotificationType.Error,
      });
    } finally {
      setUpdateState(false);
    }
  });

  return (
    <BaseDialog
      open={open}
      title={`New Version v${updateInfo?.manifest?.version}`}
      contentSx={{ minWidth: 360, maxWidth: 400, maxHeight: "50vh" }}
      okBtn={t("Update")}
      cancelBtn={t("Cancel")}
      onClose={() => setOpen(false)}
      onCancel={() => setOpen(false)}
      onOk={onUpdate}
    >
      <UpdateLog dangerouslySetInnerHTML={{ __html: parseContent }} />
    </BaseDialog>
  );
});

UpdateViewer.displayName = "UpdateViewer";
