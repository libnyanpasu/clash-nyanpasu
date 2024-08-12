import MdiTrayFull from "~icons/mdi/tray-full";
import { useLockFn } from "ahooks";
import React, { lazy } from "react";
import { useTranslation } from "react-i18next";
import HotkeyDialog from "@/components/setting/modules/hotkey-dialog";
import TrayIconDialog from "@/components/setting/modules/tray-icon-dialog";
import { formatEnvInfos } from "@/utils";
import { Feedback, GitHub, Keyboard } from "@mui/icons-material";
import { IconButton } from "@mui/material";
import { collect_envs } from "@nyanpasu/interface";
import { BasePage } from "@nyanpasu/ui";
import { open } from "@tauri-apps/api/shell";

const GithubIcon = () => {
  const toGithubRepo = useLockFn(() => {
    return open("https://github.com/LibNyanpasu/clash-nyanpasu");
  });

  return (
    <IconButton
      color="inherit"
      title="@keiko233/clash-nyanpasu"
      onClick={toGithubRepo}
    >
      <GitHub fontSize="inherit" />
    </IconButton>
  );
};

const FeedbackIcon = () => {
  const toFeedback = useLockFn(async () => {
    const envs = await collect_envs();
    const formattedEnv = encodeURIComponent(
      formatEnvInfos(envs)
        .split("\n")
        .map((v) => `> ${v}`)
        .join("\n"),
    );
    return open(
      "https://github.com/LibNyanpasu/clash-nyanpasu/issues/new?assignees=&labels=T%3A+Bug%2CS%3A+Untriaged&projects=&template=bug_report.yaml&env_infos=" +
        formattedEnv,
    );
  });
  return (
    <IconButton color="inherit" title="Feedback" onClick={toFeedback}>
      <Feedback fontSize="inherit" />
    </IconButton>
  );
};

// FIXME: it should move to a proper place
const HotkeyButton = () => {
  const [open, setOpen] = React.useState(false);
  return (
    <>
      <HotkeyDialog open={open} onClose={() => setOpen(false)} />
      <IconButton color="inherit" title="Hotkeys" onClick={() => setOpen(true)}>
        <Keyboard fontSize="inherit" />
      </IconButton>
    </>
  );
};

// FIXME: it should move to a proper place
const TrayIconButton = () => {
  const [open, setOpen] = React.useState(false);
  return (
    <>
      <TrayIconDialog open={open} onClose={() => setOpen(false)} />
      <IconButton
        color="inherit"
        title="Tray Icons"
        onClick={() => setOpen(true)}
      >
        <MdiTrayFull fontSize="inherit" />
      </IconButton>
    </>
  );
};

export default function SettingPage() {
  const { t } = useTranslation();

  const Component = lazy(() => import("@/components/setting/setting-page"));

  return (
    <BasePage
      title={t("Settings")}
      header={
        <div className="flex gap-1">
          <TrayIconButton />
          <HotkeyButton />
          <FeedbackIcon />
          <GithubIcon />
        </div>
      }
    >
      <Component />
    </BasePage>
  );
}
