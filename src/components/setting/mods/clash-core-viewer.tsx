import { BaseDialog, DialogRef } from "@/components/base";
import { useNotification } from "@/hooks/use-notification";
import { useVerge } from "@/hooks/use-verge";
import { closeAllConnections } from "@/services/api";
import {
  changeClashCore,
  fetchLatestCoreVersions,
  getCoreVersion,
  grantPermission,
  restartSidecar,
  updateCore,
} from "@/services/cmds";
import getSystem from "@/utils/get-system";
import { FiberManualRecord, Lock, Update } from "@mui/icons-material";
import { LoadingButton } from "@mui/lab";
import {
  Box,
  CircularProgress,
  IconButton,
  List,
  ListItemButton,
  ListItemText,
  alpha,
  useTheme,
} from "@mui/material";
import { useAsyncEffect, useLockFn } from "ahooks";
import { forwardRef, useImperativeHandle, useState } from "react";
import { useTranslation } from "react-i18next";
import { mutate } from "swr";

type ClashCore = Required<IVergeConfig>["clash_core"];

interface Core {
  name: string;
  core: ClashCore;
  version?: string;
  latest?: string;
}

const VALID_CORE: Core[] = [
  { name: "Clash Premium", core: "clash" },
  { name: "Mihomo", core: "mihomo" },
  { name: "Mihomo Alpha", core: "mihomo-alpha" },
  { name: "Clash Rust", core: "clash-rs" },
];

const OS = getSystem();

export const ClashCoreViewer = forwardRef<DialogRef>((props, ref) => {
  const { t } = useTranslation();

  const { verge } = useVerge();

  const [open, setOpen] = useState(false);
  const [lock, setLock] = useState(false);
  const [validCores, setValidCores] = useState<Core[]>(VALID_CORE);
  useImperativeHandle(ref, () => ({
    open: () => setOpen(true),
    close: () => setOpen(false),
  }));

  const { clash_core = "clash" } = verge ?? {};

  const [checkUpdatesLoading, setCheckUpdatesLoading] = useState(false);
  const onCheckUpdates = useLockFn(async () => {
    try {
      setCheckUpdatesLoading(true);
      const results = await fetchLatestCoreVersions();
      const buf = validCores.map((each) => ({
        ...each,
        latest:
          each.core === "clash"
            ? results["clash_premium"]
            : results[each.core.replace(/-/g, "_") as keyof typeof results],
      }));
      setValidCores(buf);
      useNotification(t("Success"), `Successfully check updates`);
    } catch (e) {
      if (e instanceof Error) {
        useNotification(t("Error"), e.message);
      } else if (typeof e === "string") {
        useNotification(t("Error"), e);
      } else {
        console.error(e);
      }
    } finally {
      setCheckUpdatesLoading(false);
    }
  });

  const [restartLoading, setRestartLoading] = useState(false);
  const onRestart = useLockFn(async () => {
    try {
      setRestartLoading(true);
      await restartSidecar();
      useNotification(t("Success"), `Successfully restart core`);
    } catch (err: any) {
      useNotification(t("Error"), err?.message || err.toString());
    } finally {
      setRestartLoading(false);
    }
  });

  useAsyncEffect(async () => {
    try {
      const versions = await Promise.all(
        VALID_CORE.reduce(
          (acc, each) => acc.concat(getCoreVersion(each.core)),
          [] as Promise<string>[],
        ),
      );
      setValidCores(
        VALID_CORE.map((each, idx) => ({
          ...each,
          version: !isNaN(Number(versions[idx][0]))
            ? `v${versions[idx]}`
            : versions[idx],
        })),
      );
    } catch (e) {
      if (e instanceof Error) {
        useNotification(t("Error"), `Failed to get core version: ${e.message}`);
      } else if (typeof e === "string") {
        useNotification(t("Error"), `Failed to get core version: ${e}`);
      } else {
        console.error(e);
      }
    }
  }, []);

  return (
    <BaseDialog
      open={open}
      title={
        <Box display="flex" gap={2}>
          {t("Clash Core")}
          <div style={{ flex: 1 }} />
          <LoadingButton
            variant="outlined"
            size="small"
            onClick={onCheckUpdates}
            loading={checkUpdatesLoading}
            disabled={checkUpdatesLoading}
          >
            {t("Check Updates")}
          </LoadingButton>

          <LoadingButton
            variant="contained"
            size="small"
            loading={restartLoading}
            onClick={onRestart}
            disabled={restartLoading}
          >
            {t("Restart")}
          </LoadingButton>
        </Box>
      }
      contentSx={{
        pb: 0,
        width: 380,
        height: 310,
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
        {validCores.map((each) => (
          <CoreElement
            lock={lock}
            key={each.core}
            selected={each.core === clash_core}
            core={each}
            onCoreChanged={(_, state) => {
              if (state === "start") setLock(true);
              else setLock(false);
            }}
          />
        ))}
      </List>
    </BaseDialog>
  );
});

ClashCoreViewer.displayName = "ClashCoreViewer";

function CoreElement({
  selected,
  core,
  lock,
  onCoreChanged,
}: {
  selected: boolean;
  core: Core;
  lock: boolean;
  onCoreChanged: (core: string, state: "start" | "finish") => void;
}) {
  const { t } = useTranslation();
  const { mutateVerge } = useVerge();
  const theme = useTheme();
  const [loading, setLoading] = useState(false);
  const needUpdate = core.latest && core.version !== core.latest;

  const onCoreChange = useLockFn(async (core: ClashCore) => {
    if (selected || lock) return;
    try {
      setLoading(true);
      onCoreChanged(core, "start");
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
    } finally {
      setLoading(false);
      onCoreChanged(core, "finish");
    }
  });

  const onGrant = useLockFn(async (core: string) => {
    try {
      await grantPermission(core);
      // 自动重启
      if (selected) await restartSidecar();
      useNotification(t("Success"), `Successfully grant permission to ${core}`);
    } catch (err: any) {
      useNotification(t("Error"), err?.message || err.toString());
    }
  });

  const [updateCoreLoading, setUpdateCoreLoading] = useState(false);
  const onUpdateCore = useLockFn(
    async (core: Required<IVergeConfig>["clash_core"]) => {
      try {
        setUpdateCoreLoading(true);
        await updateCore(core);
        mutateVerge();
        setTimeout(() => {
          mutate("getClashConfig");
          mutate("getVersion");
        }, 100);
        useNotification(t("Success"), `Successfully updated to ${core}`);
      } catch (err: any) {
        useNotification(t("Error"), err?.message || err.toString());
      } finally {
        setUpdateCoreLoading(false);
      }
    },
  );

  return (
    <ListItemButton
      selected={selected}
      onClick={() => onCoreChange(core.core)}
      style={{
        position: "relative",
      }}
      sx={{
        backgroundColor: loading
          ? alpha(theme.palette.action.focus, 0.03)
          : undefined,
      }}
    >
      <CircularProgress
        style={{
          position: "absolute",
          left: "50%",
        }}
        size="1.5em"
        color="primary"
        thickness={4}
        disableShrink={true}
        sx={{
          visibility: loading ? "visible" : "hidden",
        }}
      />

      <ListItemText
        primary={
          <div
            style={{
              display: "flex",
              alignItems: "center",
            }}
          >
            <span>{core.name}</span>

            {needUpdate && (
              <FiberManualRecord
                fontSize="small"
                color="secondary"
                style={{
                  transform: "scale(0.5)",
                }}
              />
            )}
          </div>
        }
        secondary={
          needUpdate
            ? `${core.version} (${core.latest})`
            : core.version ?? `/${core.core}`
        }
      />
      {needUpdate && (
        <IconButton
          color="inherit"
          size="small"
          onClick={(e) => {
            e.preventDefault();
            e.stopPropagation();
            onUpdateCore(core.core);
          }}
        >
          {updateCoreLoading ? (
            <CircularProgress size="1em" />
          ) : (
            <Update fontSize="inherit" />
          )}
        </IconButton>
      )}
      {(OS === "macos" || OS === "linux") && (
        <IconButton
          color="inherit"
          size="small"
          edge="end"
          onClick={(e) => {
            e.preventDefault();
            e.stopPropagation();
            onGrant(core.core);
          }}
        >
          <Lock fontSize="inherit" />
        </IconButton>
      )}
    </ListItemButton>
  );
}
