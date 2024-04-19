import { List } from "@mui/material";
import { BaseCard, MenuItem, SwitchItem } from "@nyanpasu/ui";
import { useTranslation } from "react-i18next";
import { createBooleanProps, createMenuProps } from "./modules";

export const SettingClashBase = () => {
  const { t } = useTranslation();

  return (
    <BaseCard label={t("Clash Setting")}>
      <List disablePadding>
        <SwitchItem
          label={t("Allow Lan")}
          {...createBooleanProps("allow-lan")}
        />

        <SwitchItem label={t("IPv6")} {...createBooleanProps("ipv6")} />

        <MenuItem
          label={t("Log Level")}
          {...createMenuProps("log-level", {
            options: {
              debug: "Debug",
              info: "Info",
              warning: "Warn",
              error: "Error",
              silent: "Silent",
            },
            fallbackSelect: "debug",
          })}
        />
      </List>
    </BaseCard>
  );
};

export default SettingClashBase;
