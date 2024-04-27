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

  const { nyanpasuConfig, setNyanpasuConfig } = useNyanpasu();

  const migrateAppPath = useLockFn(async () => {
    try {
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

  return (
    <BaseCard label={t("Path Manager")}>
      <Grid container spacing={2}>
        <Grid xs={6}>
          <PaperButton label={t("Open App Dir")} onClick={() => openAppDir()} />
        </Grid>

        <Grid xs={6}>
          <PaperButton
            label={t("Migration App Path")}
            onClick={() => migrateAppPath()}
          />
        </Grid>

        <Grid xs={6}>
          <PaperButton
            label={t("Open Core Dir")}
            onClick={() => openCoreDir()}
          />
        </Grid>

        <Grid xs={6}>
          <PaperButton
            label={t("Open Logs Dir")}
            onClick={() => openLogsDir()}
          />
        </Grid>

        <Grid xs={6}>
          <PaperButton
            label={t("Collect Logs")}
            onClick={() => collectLogs()}
          />
        </Grid>
      </Grid>
    </BaseCard>
  );
};

export default SettingNyanpasuPath;
