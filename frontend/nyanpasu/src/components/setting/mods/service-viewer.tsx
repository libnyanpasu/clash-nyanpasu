import { BaseDialog, DialogRef } from "@/components/base";
import { NotificationType, useNotification } from "@/hooks/use-notification";
import {
  checkService,
  installService,
  patchVergeConfig,
  uninstallService,
} from "@/services/cmds";
import { Button, Stack, Typography } from "@mui/material";
import { useLockFn } from "ahooks";
import { forwardRef, useImperativeHandle, useState } from "react";
import { useTranslation } from "react-i18next";
import useSWR from "swr";

interface Props {
  enable: boolean;
}

export const ServiceViewer = forwardRef<DialogRef, Props>((props, ref) => {
  const { enable } = props;

  const { t } = useTranslation();
  const [open, setOpen] = useState(false);

  const { data: status, mutate: mutateCheck } = useSWR(
    "checkService",
    checkService,
    {
      revalidateIfStale: false,
      shouldRetryOnError: false,
      focusThrottleInterval: 36e5, // 1 hour
    },
  );

  useImperativeHandle(ref, () => ({
    open: () => setOpen(true),
    close: () => setOpen(false),
  }));

  const state = status != null ? status : "pending";

  const onInstall = useLockFn(async () => {
    try {
      await installService();
      mutateCheck();
      setOpen(false);
      useNotification({
        title: t("Success"),
        body: "Service installed successfully",
        type: NotificationType.Success,
      });
    } catch (err: any) {
      mutateCheck();
      useNotification({
        title: t("Error"),
        body: err.message || err.toString(),
        type: NotificationType.Error,
      });
    }
  });

  const onUninstall = useLockFn(async () => {
    try {
      if (enable) {
        await patchVergeConfig({ enable_service_mode: false });
      }

      await uninstallService();
      mutateCheck();
      setOpen(false);
      useNotification({
        title: t("Success"),
        body: "Service uninstalled successfully",
        type: NotificationType.Success,
      });
    } catch (err: any) {
      mutateCheck();
      useNotification({
        title: t("Error"),
        body: err.message || err.toString(),
        type: NotificationType.Error,
      });
    }
  });

  // fix unhandled error of the service mode
  const onDisable = useLockFn(async () => {
    try {
      await patchVergeConfig({ enable_service_mode: false });
      mutateCheck();
      setOpen(false);
    } catch (err: any) {
      mutateCheck();
      useNotification({
        title: t("Error"),
        body: err.message || err.toString(),
        type: NotificationType.Error,
      });
    }
  });

  return (
    <BaseDialog
      open={open}
      title={t("Service Mode")}
      contentSx={{ width: 360, userSelect: "text" }}
      disableFooter
      onClose={() => setOpen(false)}
    >
      <Typography>Current State: {state}</Typography>

      {(state === "unknown" || state === "uninstall") && (
        <Typography>
          Information: Please make sure that the Clash Nyanpasu Service is
          installed and enabled
        </Typography>
      )}

      <Stack
        direction="row"
        spacing={1}
        sx={{ mt: 4, justifyContent: "flex-end" }}
      >
        {state === "uninstall" && enable && (
          <Button variant="contained" onClick={onDisable}>
            Disable Service Mode
          </Button>
        )}

        {state === "uninstall" && (
          <Button variant="contained" onClick={onInstall}>
            Install
          </Button>
        )}

        {(state === "active" || state === "installed") && (
          <Button variant="outlined" onClick={onUninstall}>
            Uninstall
          </Button>
        )}
      </Stack>
    </BaseDialog>
  );
});

ServiceViewer.displayName = "ServiceViewer";
