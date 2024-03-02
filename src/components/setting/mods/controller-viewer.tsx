import { BaseDialog, DialogRef } from "@/components/base";
import { useClashInfo } from "@/hooks/use-clash";
import { NotificationType, useNotification } from "@/hooks/use-notification";
import { useVerge } from "@/hooks/use-verge";
import {
  List,
  ListItem,
  ListItemText,
  MenuItem,
  Select,
  TextField,
} from "@mui/material";
import { useLockFn } from "ahooks";
import { forwardRef, useImperativeHandle, useState } from "react";
import { useTranslation } from "react-i18next";

export const ControllerViewer = forwardRef<DialogRef>((props, ref) => {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [loading, setLoading] = useState(false);

  const { clashInfo, patchInfo } = useClashInfo();
  const { verge, patchVerge } = useVerge();
  const [controller, setController] = useState(clashInfo?.server || "");
  const [secret, setSecret] = useState(clashInfo?.secret || "");
  const [portStrategy, setPortStrategy] = useState(
    verge?.clash_strategy?.external_controller_port_strategy ||
      "allow_fallback",
  );

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
      await patchVerge({
        clash_strategy: { external_controller_port_strategy: portStrategy },
      });
      await patchInfo({ "external-controller": controller, secret });
      useNotification({
        title: t("Success"),
        body: t("Change Clash Config successfully!"),
        type: NotificationType.Success,
      });
      setOpen(false);
    } catch (err: any) {
      useNotification({
        title: t("Error"),
        body: err.message || err.toString(),
        type: NotificationType.Error,
      });
    } finally {
      setLoading(false);
    }
  });

  return (
    <BaseDialog
      open={open}
      title={t("Clash External Controll")}
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
          <ListItemText primary="Port Strategy" />
          <Select
            size="small"
            sx={{ width: 175 }}
            defaultValue="allow_fallback"
            value={portStrategy}
            onChange={(e) =>
              setPortStrategy(e.target.value as typeof portStrategy)
            }
          >
            <MenuItem value="allow_fallback">Allow Fallback</MenuItem>
            <MenuItem value="fixed">Fixed</MenuItem>
            <MenuItem value="random">Random</MenuItem>
          </Select>
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
