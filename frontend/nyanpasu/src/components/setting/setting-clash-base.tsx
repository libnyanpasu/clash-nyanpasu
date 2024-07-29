import { useTranslation } from "react-i18next";
import { useMessage } from "@/hooks/use-notification";
import getSystem from "@/utils/get-system";
import { Button, List, ListItem, ListItemText } from "@mui/material";
import { pullupUWPTool } from "@nyanpasu/interface";
import { BaseCard, MenuItem, SwitchItem } from "@nyanpasu/ui";
import { clash } from "./modules";

const { createBooleanProps, createMenuProps } = clash;

const isWIN = getSystem() === "windows";

export const SettingClashBase = () => {
  const { t } = useTranslation();

  const clickUWP = async () => {
    try {
      await pullupUWPTool();
    } catch (e) {
      useMessage(`Failed to Open UWP Tools.\n${JSON.stringify(e)}`, {
        title: t("Error"),
        type: "error",
      });
    }
  };

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

        {isWIN && (
          <ListItem sx={{ pl: 0, pr: 0 }}>
            <ListItemText primary={t("Open UWP tool")} />

            <Button variant="contained" onClick={clickUWP}>
              Open
            </Button>
          </ListItem>
        )}
      </List>
    </BaseCard>
  );
};

export default SettingClashBase;
