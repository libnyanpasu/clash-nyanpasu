import { BaseDialog, DialogRef } from "@/components/base";
import { useMessage } from "@/hooks/use-notification";
import { isPortable } from "@/services/cmds";
import { atomUpdateState } from "@/services/states";
import { relaunch } from "@tauri-apps/api/process";
import { checkUpdate, installUpdate } from "@tauri-apps/api/updater";
import { open as openWebUrl } from "@tauri-apps/api/shell";
import { useLockFn } from "ahooks";
import { forwardRef, useImperativeHandle, useState } from "react";
import { useTranslation } from "react-i18next";
import { useRecoilState } from "recoil";
import Markdown, { Components } from "react-markdown";
import useSWR from "swr";
import { Chip, Tooltip } from "@mui/material";

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

  const components: Components = {
    a: ({ children, href }) => {
      const click = () => openWebUrl(href || "");

      return (
        <Tooltip title="Show on GitHub">
          <Chip
            label={children}
            size="small"
            sx={{ height: "20px" }}
            onClick={click}
          />
        </Tooltip>
      );
    },
  };

  const updatePreprocess = () => {
    const context = updateInfo?.manifest?.body;

    if (!context) {
      return "New Version is available";
    }

    return context.replace(/@(\w+)/g, "[@$1](https://github.com/$1)");
  };

  const onUpdate = useLockFn(async () => {
    const portable = await isPortable();
    if (portable) {
      useMessage(t("Portable Update Error"), {
        type: "error",
        title: t("Error"),
      });
      return;
    }
    if (updateState) return;
    setUpdateState(true);

    try {
      await installUpdate();
      await relaunch();
    } catch (err: any) {
      useMessage(err.message || err.toString(), {
        type: "error",
        title: t("Error"),
      });
    } finally {
      setUpdateState(false);
    }
  });

  return (
    <BaseDialog
      open={open}
      title={`New Version v${updateInfo?.manifest?.version}`}
      contentSx={{ minWidth: 360, maxWidth: 400, maxHeight: "80%" }}
      okBtn={t("Update")}
      cancelBtn={t("Cancel")}
      onClose={() => setOpen(false)}
      onCancel={() => setOpen(false)}
      onOk={onUpdate}
    >
      {/* <UpdateLog dangerouslySetInnerHTML={{ __html: parseContent }} /> */}
      <Markdown components={components}>{updatePreprocess()}</Markdown>
    </BaseDialog>
  );
});

UpdateViewer.displayName = "UpdateViewer";
