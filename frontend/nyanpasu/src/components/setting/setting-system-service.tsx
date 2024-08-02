import { useMemoizedFn } from "ahooks";
import { ChangeEvent, useTransition } from "react";
import { useTranslation } from "react-i18next";
import { message } from "@/utils/notification";
import { LoadingButton } from "@mui/lab";
import { List, ListItem, ListItemText, Typography } from "@mui/material";
import { restartSidecar, useNyanpasu } from "@nyanpasu/interface";
import { BaseCard, SwitchItem } from "@nyanpasu/ui";
import { nyanpasu } from "./modules/create-props";

const { useBooleanProps: createBooleanProps } = nyanpasu;

export const SettingSystemService = () => {
  const { t } = useTranslation();

  const { getServiceStatus, setServiceStatus } = useNyanpasu();

  const getInstallButtonString = () => {
    switch (getServiceStatus.data) {
      case "running":
      case "stopped": {
        return "Uninstall";
      }

      case "not_installed": {
        return "Install";
      }
    }
  };
  const getControlButtonString = () => {
    switch (getServiceStatus.data) {
      case "running": {
        return "Stop";
      }

      case "stopped": {
        return "Start";
      }
    }
  };

  const isDisabled = getServiceStatus.data === "not_installed";

  const [installOrUninstallPending, startInstallOrUninstall] = useTransition();
  const handleInstallClick = useMemoizedFn(() => {
    startInstallOrUninstall(async () => {
      try {
        switch (getServiceStatus.data) {
          case "running":
          case "stopped":
            await setServiceStatus("uninstall");
            break;

          case "not_installed":
            await setServiceStatus("install");
            break;

          default:
            break;
        }
        await restartSidecar();
      } catch (e) {
        const errorMessage =
          getServiceStatus.data === "not_installed"
            ? "Install failed"
            : "Uninstall failed";

        message(errorMessage, {
          type: "error",
          title: t("Error"),
        });
      }
    });
  });

  const [serviceControlPending, startServiceControl] = useTransition();
  const handleControlClick = useMemoizedFn(() => {
    startServiceControl(async () => {
      try {
        switch (getServiceStatus.data) {
          case "running":
            await setServiceStatus("stop");
            break;

          case "stopped":
            await setServiceStatus("start");
            break;

          default:
            break;
        }
        await restartSidecar();
      } catch (e) {
        const errorMessage =
          getServiceStatus.data === "running" ? "Stop failed" : "Start failed";

        message(errorMessage, {
          type: "error",
          title: t("Error"),
        });
      }
    });
  });
  const serviceToggleProps = createBooleanProps("enable_service_mode");
  const onChange = async (
    event: ChangeEvent<HTMLInputElement>,
    checked: boolean,
  ) => {
    await serviceToggleProps.onChange?.(event, checked);
    await restartSidecar();
  };

  return (
    <BaseCard label="System Service">
      <List disablePadding>
        <SwitchItem
          label={t("Service Mode")}
          disabled={isDisabled}
          {...serviceToggleProps}
          onChange={onChange}
        />

        {isDisabled && (
          <ListItem sx={{ pl: 0, pr: 0 }}>
            <Typography>
              Information: Please make sure that the Clash Nyanpasu Service is
              installed and enabled
            </Typography>
          </ListItem>
        )}

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText primary={`Current State: ${getServiceStatus.data}`} />
          <div className="flex gap-2">
            {!isDisabled && (
              <LoadingButton
                variant="contained"
                onClick={handleControlClick}
                loading={serviceControlPending}
                disabled={installOrUninstallPending || serviceControlPending}
              >
                {getControlButtonString()}
              </LoadingButton>
            )}

            <LoadingButton
              variant="contained"
              onClick={handleInstallClick}
              loading={installOrUninstallPending}
              disabled={installOrUninstallPending || serviceControlPending}
            >
              {getInstallButtonString()}
            </LoadingButton>
          </div>
        </ListItem>
      </List>
    </BaseCard>
  );
};

export default SettingSystemService;
