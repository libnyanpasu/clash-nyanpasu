import { BaseDialog, DialogRef } from "@/components/base";
import { useClashInfo } from "@/hooks/use-clash";
import { useNotification } from "@/hooks/use-notification";
import { List, ListItem, ListItemText, TextField } from "@mui/material";
import { useLockFn } from "ahooks";
import { forwardRef, useImperativeHandle, useState } from "react";
import { useTranslation } from "react-i18next";

export const ControllerViewer = forwardRef<DialogRef>((props, ref) => {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [loading, setLoading] = useState(false);

  const { clashInfo, patchInfo } = useClashInfo();

  const [controller, setController] = useState(clashInfo?.server || "");
  const [secret, setSecret] = useState(clashInfo?.secret || "");

  useImperativeHandle(ref, () => ({
    open: () => {
      setOpen(true);
      setController(clashInfo?.server || "");
      setSecret(clashInfo?.secret || "");
    },
    close: () => setOpen(false),
  }));

  const onSave = useLockFn(async () => {
    try {
      setLoading(true);
      await patchInfo({ "external-controller": controller, secret });
      useNotification(t("Success"), "Change Clash Config successfully!");
      setOpen(false);
    } catch (err: any) {
      useNotification(t("Error"), err.message || err.toString());
    } finally {
      setLoading(false);
    }
  });

  return (
    <BaseDialog
      open={open}
      title={t("Clash Port")}
      contentSx={{ width: 400 }}
      okBtn={t("Save")}
      cancelBtn={t("Cancel")}
      onClose={() => setOpen(false)}
      onCancel={() => setOpen(false)}
      loading={loading}
      onOk={onSave}
    >
      <List>
        <ListItem sx={{ padding: "5px 2px" }}>
          <ListItemText primary="External Controller" />
          <TextField
            size="small"
            autoComplete="off"
            sx={{ width: 175 }}
            value={controller}
            placeholder="Required"
            onChange={(e) => setController(e.target.value)}
          />
        </ListItem>

        <ListItem sx={{ padding: "5px 2px" }}>
          <ListItemText primary="Core Secret" />
          <TextField
            size="small"
            autoComplete="off"
            sx={{ width: 175 }}
            value={secret}
            placeholder="Recommended"
            onChange={(e) => setSecret(e.target.value)}
          />
        </ListItem>
      </List>
    </BaseDialog>
  );
});

ControllerViewer.displayName = "ControllerViewer";
