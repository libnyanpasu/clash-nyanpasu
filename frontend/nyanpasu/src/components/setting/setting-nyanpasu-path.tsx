import { useLockFn } from "ahooks";
import { useTranslation } from "react-i18next";
import { sleep } from "@/utils";
import { message } from "@/utils/notification";
import Grid from "@mui/material/Unstable_Grid2";
import {
  collectLogs,
  openAppConfigDir,
  openAppDataDir,
  openCoreDir,
  openLogsDir,
  restartApplication,
  setCustomAppDir,
} from "@nyanpasu/interface";
import { BaseCard } from "@nyanpasu/ui";
import { open } from "@tauri-apps/api/dialog";
import { PaperButton } from "./modules/nyanpasu-path";

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
        message(t("Multiple directories are not supported"), {
          title: t("Error"),
          type: "error",
        });

        return;
      }

      await setCustomAppDir(selected);

      message(t("App directory changed successfully"), {
        title: t("Success"),
        type: "error",
      });

      await sleep(1000);

      await restartApplication();
    } catch (e) {
      message(`Migration failed! ${JSON.stringify(e)}`, {
        title: t("Error"),
        type: "error",
      });
    }
  });

  const gridLists = [
    { label: t("Open Config Dir"), onClick: openAppConfigDir },
    { label: t("Open Data Dir"), onClick: openAppDataDir },
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
