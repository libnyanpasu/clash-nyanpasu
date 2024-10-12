import { useMemoizedFn } from "ahooks";
import { ChangeEvent, useTransition } from "react";
import { useTranslation } from "react-i18next";
import { formatError } from "@/utils";
import { message } from "@/utils/notification";
import { LoadingButton } from "@mui/lab";
import { List, ListItem, ListItemText, Typography } from "@mui/material";
import { restartSidecar, useNyanpasu } from "@nyanpasu/interface";
import { BaseCard, SwitchItem } from "@nyanpasu/ui";
import { nyanpasu } from "./modules/create-props";
import {
  ServerManualPromptDialogWrapper,
  useServerManualPromptDialog,
} from "./modules/service-manual-prompt-dialog";

const { useBooleanProps: createBooleanProps } = nyanpasu;

export const SettingSystemService = () => {
  const { t } = useTranslation();

  const { getServiceStatus, setServiceStatus } = useNyanpasu();

  const getInstallButtonString = () => {
    switch (getServiceStatus.data) {
      case "running":
      case "stopped": {
        return t("Uninstall");
      }

      case "not_installed": {
        return t("Install");
      }
    }
  };
  const getControlButtonString = () => {
    switch (getServiceStatus.data) {
      case "running": {
        return t("Stop");
      }

      case "stopped": {
        return t("Start");
      }
    }
  };

  const isDisabled = getServiceStatus.data === "not_installed";

  const promptDialog = useServerManualPromptDialog();

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
        const errorMessage = `${
          getServiceStatus.data === "not_installed"
            ? t("Install failed")
            : t("Uninstall failed")
        }: ${formatError(e)}`;

        message(errorMessage, {
          kind: "error",
          title: t("Error"),
        });
        // If install failed show a prompt to user to install the service manually
        promptDialog.show(
          getServiceStatus.data === "not_installed" ? "install" : "uninstall",
        );
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
          getServiceStatus.data === "running"
            ? `Stop failed: ${formatError(e)}`
            : `Start failed: ${formatError(e)}`;

        message(errorMessage, {
          kind: "error",
          title: t("Error"),
        });
        // If start failed show a prompt to user to start the service manually
        promptDialog.show(
          getServiceStatus.data === "running" ? "stop" : "start",
        );
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
    <BaseCard label={t("System Service")}>
      <ServerManualPromptDialogWrapper />
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
              {t(
                "Information: Please make sure that the Clash Nyanpasu Service is installed and enabled.",
              )}
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
