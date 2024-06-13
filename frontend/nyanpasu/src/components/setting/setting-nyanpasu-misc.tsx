import { List } from "@mui/material";
import { BaseCard, MenuItem, SwitchItem, TextItem } from "@nyanpasu/ui";
import { useTranslation } from "react-i18next";
import { nyanpasu } from "./modules/create-props";
import { useNyanpasu } from "@nyanpasu/interface";

const { createBooleanProps } = nyanpasu;

export const SettingNyanpasuMisc = () => {
  const { t } = useTranslation();

  const { nyanpasuConfig, setNyanpasuConfig } = useNyanpasu();

  const logOptions = {
    trace: "Trace",
    debug: "Debug",
    info: "Info",
    warn: "Warn",
    error: "Error",
    silent: "Silent",
  };

  return (
    <BaseCard label={t("Nyanpasu Setting")}>
      <List disablePadding>
        <MenuItem
          label={t("App Log Level")}
          options={logOptions}
          selected={nyanpasuConfig?.app_log_level || "info"}
          onSelected={(value) =>
            setNyanpasuConfig({ app_log_level: value as string })
          }
        />

        <SwitchItem
          label={t("Auto Close Connections")}
          {...createBooleanProps("auto_close_connection")}
        />

        <SwitchItem
          label={t("Enable Builtin Enhanced")}
          {...createBooleanProps("enable_builtin_enhanced")}
        />

        <SwitchItem
          label={t("Enable Tray Proxies Selector")}
          {...createBooleanProps("clash_tray_selector")}
        />

        <SwitchItem
          label={t("Lighten up Animation Effects")}
          {...createBooleanProps("lighten_animation_effects")}
        />

        <TextItem
          label={t("Default Latency Test")}
          placeholder="http://www.gstatic.com/generate_204"
          value={nyanpasuConfig?.default_latency_test || ""}
          onApply={(value) =>
            setNyanpasuConfig({ default_latency_test: value })
          }
        />
      </List>
    </BaseCard>
  );
};

export default SettingNyanpasuMisc;
