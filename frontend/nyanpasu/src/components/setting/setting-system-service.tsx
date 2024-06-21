import { useMessage } from "@/hooks/use-notification";
import {
  Button,
  List,
  ListItem,
  ListItemText,
  Typography,
} from "@mui/material";
import { useNyanpasu } from "@nyanpasu/interface";
import { BaseCard, SwitchItem } from "@nyanpasu/ui";
import { useTranslation } from "react-i18next";
import { nyanpasu } from "./modules/create-props";

const { createBooleanProps } = nyanpasu;

export const SettingSystemService = () => {
  const { t } = useTranslation();

  const { getServiceStatus, setServiceStatus } = useNyanpasu();

  const getButtonString = () => {
    switch (getServiceStatus.data) {
      case "unknown":
      case "installed":
      case "active": {
        return "Uninstall";
      }

      case "uninstall": {
        return "Install";
      }
    }
  };

  const checkDisbale =
    getServiceStatus.data === "unknown" ||
    getServiceStatus.data === "uninstall";

  const handleClick = async () => {
    try {
      switch (getServiceStatus.data) {
        case "unknown":
        case "installed":
        case "active":
          setServiceStatus("uninstall");
          break;

        case "uninstall":
          setServiceStatus("install");
          break;

        default:
          break;
      }
    } catch (e) {
      const errorMessage =
        getServiceStatus.data === "uninstall"
          ? "Install failed"
          : "Uninstall failed";

      useMessage(errorMessage, {
        type: "error",
        title: t("Error"),
      });
    }
  };

  return (
    <BaseCard label="System Service">
      <List disablePadding>
        <SwitchItem
          label={t("Service Mode")}
          disabled={checkDisbale}
          {...createBooleanProps("enable_service_mode")}
        />

        {checkDisbale && (
          <ListItem sx={{ pl: 0, pr: 0 }}>
            <Typography>
              Information: Please make sure that the Clash Nyanpasu Service is
              installed and enabled
            </Typography>
          </ListItem>
        )}

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText primary={`Current State: ${getServiceStatus.data}`} />

          <Button variant="contained" onClick={handleClick}>
            {getButtonString()}
          </Button>
        </ListItem>
      </List>
    </BaseCard>
  );
};

export default SettingSystemService;
