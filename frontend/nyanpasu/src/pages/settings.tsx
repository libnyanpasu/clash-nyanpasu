import { BasePage } from "@/components/base";
import SettingClash from "@/components/setting/setting-clash";
import SettingSystem from "@/components/setting/setting-system";
import SettingVerge from "@/components/setting/setting-verge";
import { NotificationType, useNotification } from "@/hooks/use-notification";
import { openWebUrl } from "@/services/cmds";
import { GitHub } from "@mui/icons-material";
import { Grid, IconButton, Paper } from "@mui/material";
import { useLockFn } from "ahooks";
import { useTranslation } from "react-i18next";

export default function SettingPage() {
  const { t } = useTranslation();

  const onError = (err: any) => {
    useNotification({
      title: t("Error"),
      body: err.message || err.toString(),
      type: NotificationType.Error,
    });
  };

  const toGithubRepo = useLockFn(() => {
    return openWebUrl("https://github.com/LibNyanpasu/clash-nyanpasu");
  });

  return (
    <BasePage
      title={t("Settings")}
      header={
        <IconButton
          size="small"
          color="inherit"
          title="@keiko233/clash-nyanpasu"
          onClick={toGithubRepo}
        >
          <GitHub fontSize="inherit" />
        </IconButton>
      }
    >
      <Grid container spacing={{ xs: 2, lg: 3 }}>
        <Grid item xs={12} md={6}>
          <Paper sx={{ borderRadius: 1, boxShadow: 2 }}>
            <SettingClash onError={onError} />
          </Paper>
        </Grid>

        <Grid item xs={12} md={6}>
          <Paper sx={{ borderRadius: 1, boxShadow: 2 }}>
            <SettingSystem onError={onError} />
          </Paper>
        </Grid>

        <Grid item xs={12} md={6}>
          <Paper sx={{ borderRadius: 1, boxShadow: 2 }}>
            <SettingVerge onError={onError} />
          </Paper>
        </Grid>
      </Grid>
    </BasePage>
  );
}
