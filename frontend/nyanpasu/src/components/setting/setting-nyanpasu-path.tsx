import {
  collectLogs,
  openAppDir,
  openCoreDir,
  openLogsDir,
  restartApplication,
  setCustomAppDir,
  useNyanpasu,
} from "@nyanpasu/interface";
import { BaseCard } from "@nyanpasu/ui";
import { useTranslation } from "react-i18next";
import Grid from "@mui/material/Unstable_Grid2";
import { PaperButton } from "./modules/nyanpasu-path";
import { useLockFn } from "ahooks";
import { open } from "@tauri-apps/api/dialog";
import { useMessage } from "@/hooks/use-notification";
import { sleep } from "@/utils";

export const SettingNyanpasuPath = () => {
  const { t } = useTranslation();

  const migrateAppPath = useLockFn(async () => {
    try {
      // TODO: use current app dir as defaultPath
      const selected = await open({
        directory: true,
        multiple: false,
      });

      // user cancelled the selection
      if (!selected) {
        return;
      }

      if (Array.isArray(selected)) {
        useMessage(t("Multiple directories are not supported"), {
          title: t("Error"),
          type: "error",
        });

        return;
      }

      await setCustomAppDir(selected);

      useMessage(t("App directory changed successfully"), {
        title: t("Success"),
        type: "error",
      });

      await sleep(1000);

      await restartApplication();
    } catch (e) {
      useMessage(`Migration failed! ${JSON.stringify(e)}`, {
        title: t("Error"),
        type: "error",
      });
    }
  });

  const gridLists = [
    { label: t("Open App Dir"), onClick: openAppDir },
    { label: t("Migration App Path"), onClick: migrateAppPath },
    { label: t("Open Core Dir"), onClick: openCoreDir },
    { label: t("Open Logs Dir"), onClick: openLogsDir },
    { label: t("Collect Logs"), onClick: collectLogs },
  ];

  return (
    <BaseCard label={t("Path Config")}>
      <Grid container alignItems="stretch" spacing={2}>
        {gridLists.map(({ label, onClick }, index) => (
          <Grid key={index} xs={6} xl={3}>
            <PaperButton
              label={label}
              onClick={onClick}
              sxPaper={{ height: "100%" }}
              sxButton={{ height: "100%" }}
            />
          </Grid>
        ))}
      </Grid>
    </BaseCard>
  );
};

export default SettingNyanpasuPath;
