import { BaseDialog, DialogRef } from "@/components/base";
import { useNotification } from "@/hooks/use-notification";
import { useVerge } from "@/hooks/use-verge";
import { closeAllConnections } from "@/services/api";
import {
  changeClashCore,
  grantPermission,
  restartSidecar,
} from "@/services/cmds";
import getSystem from "@/utils/get-system";
import { Lock } from "@mui/icons-material";
import {
  Box,
  Button,
  IconButton,
  List,
  ListItemButton,
  ListItemText,
} from "@mui/material";
import { useLockFn } from "ahooks";
import { forwardRef, useImperativeHandle, useState } from "react";
import { useTranslation } from "react-i18next";
import { mutate } from "swr";

const VALID_CORE = [
  { name: "Clash", core: "clash" },
  { name: "Clash Meta", core: "clash-meta" },
  { name: "Clash Rust", core: "clash-rs" },
];

const OS = getSystem();

export const ClashCoreViewer = forwardRef<DialogRef>((props, ref) => {
  const { t } = useTranslation();

  const { verge, mutateVerge } = useVerge();

  const [open, setOpen] = useState(false);

  useImperativeHandle(ref, () => ({
    open: () => setOpen(true),
    close: () => setOpen(false),
  }));

  const { clash_core = "clash" } = verge ?? {};

  const onCoreChange = useLockFn(async (core: string) => {
    if (core === clash_core) return;

    try {
      closeAllConnections();
      await changeClashCore(core);
      mutateVerge();
      setTimeout(() => {
        mutate("getClashConfig");
        mutate("getVersion");
      }, 100);
      useNotification(t("Success"), `Successfully switch to ${core}`);
    } catch (err: any) {
      useNotification(t("Error"), err?.message || err.toString());
    }
  });

  const onGrant = useLockFn(async (core: string) => {
    try {
      await grantPermission(core);
      // 自动重启
      if (core === clash_core) await restartSidecar();
      useNotification(t("Success"), `Successfully grant permission to ${core}`);
    } catch (err: any) {
      useNotification(t("Error"), err?.message || err.toString());
    }
  });

  const onRestart = useLockFn(async () => {
    try {
      await restartSidecar();
      useNotification(t("Success"), `Successfully restart core`);
    } catch (err: any) {
      useNotification(t("Error"), err?.message || err.toString());
    }
  });

  return (
    <BaseDialog
      open={open}
      title={
        <Box display="flex" justifyContent="space-between">
          {t("Clash Core")}

          <Button variant="contained" size="small" onClick={onRestart}>
            {t("Restart")}
          </Button>
        </Box>
      }
      contentSx={{
        pb: 0,
        width: 320,
        height: 200,
        overflowY: "auto",
        userSelect: "text",
        marginTop: "-8px",
      }}
      disableOk
      cancelBtn={t("Back")}
      onClose={() => setOpen(false)}
      onCancel={() => setOpen(false)}
    >
      <List component="nav">
        {VALID_CORE.map((each) => (
          <ListItemButton
            key={each.core}
            selected={each.core === clash_core}
            onClick={() => onCoreChange(each.core)}
          >
            <ListItemText primary={each.name} secondary={`/${each.core}`} />

            {(OS === "macos" || OS === "linux") && (
              <IconButton
                color="inherit"
                size="small"
                edge="end"
                onClick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  onGrant(each.core);
                }}
              >
                <Lock fontSize="inherit" />
              </IconButton>
            )}
          </ListItemButton>
        ))}
      </List>
    </BaseDialog>
  );
});

ClashCoreViewer.displayName = "ClashCoreViewer";
