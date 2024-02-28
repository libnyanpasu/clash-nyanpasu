import { DialogRef } from "@/components/base";
import { NotificationType, useNotification } from "@/hooks/use-notification";
import { useVerge } from "@/hooks/use-verge";
import {
  collectLogs,
  openAppDir,
  openCoreDir,
  openLogsDir,
} from "@/services/cmds";
import getSystem from "@/utils/get-system";
import { ArrowForward, IosShare } from "@mui/icons-material";
import {
  IconButton,
  MenuItem,
  Select,
  Tooltip,
  Typography,
} from "@mui/material";
import { version } from "@root/package.json";
import { checkUpdate } from "@tauri-apps/api/updater";
import { useLockFn } from "ahooks";
import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ConfigViewer } from "./mods/config-viewer";
import { GuardState } from "./mods/guard-state";
import { HotkeyViewer } from "./mods/hotkey-viewer";
import { LayoutViewer } from "./mods/layout-viewer";
import { MiscViewer } from "./mods/misc-viewer";
import { SettingItem, SettingList } from "./mods/setting-comp";
import { TasksViewer } from "./mods/tasks-viewer";
import { ThemeModeSwitch } from "./mods/theme-mode-switch";
import { ThemeViewer } from "./mods/theme-viewer";
import { UpdateViewer } from "./mods/update-viewer";

interface Props {
  onError?: (err: Error) => void;
}

const OS = getSystem();

const SettingVerge = ({ onError }: Props) => {
  const { t } = useTranslation();

  const { verge, patchVerge, mutateVerge } = useVerge();
  const { theme_mode, language } = verge ?? {};

  const [loading, setLoading] = useState({
    theme_mode: false,
    language: false,
  });

  const configRef = useRef<DialogRef>(null);
  const hotkeyRef = useRef<DialogRef>(null);
  const miscRef = useRef<DialogRef>(null);
  const themeRef = useRef<DialogRef>(null);
  const layoutRef = useRef<DialogRef>(null);
  const updateRef = useRef<DialogRef>(null);
  const tasksRef = useRef<DialogRef>(null);

  const onCheckUpdate = useLockFn(async () => {
    try {
      const info = await checkUpdate();
      if (!info?.shouldUpdate) {
        useNotification({
          title: t("Success"),
          body: t("No updates available"),
        });
      } else {
        updateRef.current?.open();
      }
    } catch (err: any) {
      useNotification({
        title: t("Error"),
        body: err.message || err.toString(),
        type: NotificationType.Error,
      });
    }
  });

  return (
    <SettingList title={t("Nyanpasu Setting")}>
      <ThemeViewer ref={themeRef} />
      <ConfigViewer ref={configRef} />
      <HotkeyViewer ref={hotkeyRef} />
      <MiscViewer ref={miscRef} />
      <LayoutViewer ref={layoutRef} />
      <UpdateViewer ref={updateRef} />
      <TasksViewer ref={tasksRef} />

      <SettingItem label={t("Language")}>
        <GuardState
          value={language ?? "en"}
          onCatch={onError}
          onFormat={(e: any) => e.target.value}
          onGuard={(e) => patchVerge({ language: e })}
          loading={loading["language"]}
        >
          <Select size="small" sx={{ width: 100, "> div": { py: "7.5px" } }}>
            <MenuItem value="zh">中文</MenuItem>
            <MenuItem value="en">English</MenuItem>
            <MenuItem value="ru">Русский</MenuItem>
          </Select>
        </GuardState>
      </SettingItem>

      <SettingItem label={t("Theme Mode")}>
        <GuardState
          value={theme_mode}
          onCatch={onError}
          onGuard={(e) => patchVerge({ theme_mode: e })}
          loading={loading["theme_mode"]}
        >
          <ThemeModeSwitch />
        </GuardState>
      </SettingItem>

      <SettingItem label={t("Theme Setting")}>
        <IconButton
          color="inherit"
          size="small"
          sx={{ my: "2px" }}
          onClick={() => themeRef.current?.open()}
        >
          <ArrowForward />
        </IconButton>
      </SettingItem>

      <SettingItem label={t("Layout Setting")}>
        <IconButton
          color="inherit"
          size="small"
          sx={{ my: "2px" }}
          onClick={() => layoutRef.current?.open()}
        >
          <ArrowForward />
        </IconButton>
      </SettingItem>

      <SettingItem label={t("Tasks")}>
        <IconButton
          color="inherit"
          size="small"
          sx={{ my: "2px" }}
          onClick={() => tasksRef.current?.open()}
        >
          <ArrowForward />
        </IconButton>
      </SettingItem>

      <SettingItem label={t("Miscellaneous")}>
        <IconButton
          color="inherit"
          size="small"
          sx={{ my: "2px" }}
          onClick={() => miscRef.current?.open()}
        >
          <ArrowForward />
        </IconButton>
      </SettingItem>

      <SettingItem label={t("Hotkey Setting")}>
        <IconButton
          color="inherit"
          size="small"
          sx={{ my: "2px" }}
          onClick={() => hotkeyRef.current?.open()}
        >
          <ArrowForward />
        </IconButton>
      </SettingItem>

      <SettingItem label={t("Runtime Config")}>
        <IconButton
          color="inherit"
          size="small"
          sx={{ my: "2px" }}
          onClick={() => configRef.current?.open()}
        >
          <ArrowForward />
        </IconButton>
      </SettingItem>

      <SettingItem label={t("Open App Dir")}>
        <IconButton
          color="inherit"
          size="small"
          sx={{ my: "2px" }}
          onClick={openAppDir}
        >
          <ArrowForward />
        </IconButton>
      </SettingItem>

      <SettingItem label={t("Open Core Dir")}>
        <IconButton
          color="inherit"
          size="small"
          sx={{ my: "2px" }}
          onClick={openCoreDir}
        >
          <ArrowForward />
        </IconButton>
      </SettingItem>

      <SettingItem
        label={t("Open Logs Dir")}
        extra={
          <Tooltip title={t("Collect Logs")}>
            <IconButton
              color="inherit"
              size="small"
              onClick={() => {
                collectLogs();
              }}
            >
              <IosShare
                fontSize="inherit"
                style={{ cursor: "pointer", opacity: 0.75 }}
              />
            </IconButton>
          </Tooltip>
        }
      >
        <IconButton
          color="inherit"
          size="small"
          sx={{ my: "2px" }}
          onClick={openLogsDir}
        >
          <ArrowForward />
        </IconButton>
      </SettingItem>

      {!(OS === "windows" && WIN_PORTABLE) && (
        <SettingItem label={t("Check for Updates")}>
          <IconButton
            color="inherit"
            size="small"
            sx={{ my: "2px" }}
            onClick={onCheckUpdate}
          >
            <ArrowForward />
          </IconButton>
        </SettingItem>
      )}

      <SettingItem label={t("Nyanpasu Version")}>
        <Typography sx={{ py: "7px", pr: 1 }}>v{version}</Typography>
      </SettingItem>
    </SettingList>
  );
};

export default SettingVerge;
