import {
  NotificationType,
  useMessage,
  useNotification,
} from "@/hooks/use-notification";
import Paper from "@mui/material/Paper";
import { BaseCard } from "@nyanpasu/ui";
import { lazy, Suspense, useState } from "react";
import { useTranslation } from "react-i18next";
import { LabelSwitch } from "./modules/clash-field";
import Box from "@mui/material/Box";

export const SettingLegacy = () => {
  const { t } = useTranslation();

  const [show, setShow] = useState(false);

  const legacyComponents = [
    () => import("./setting-clash"),
    () => import("./setting-system"),
    () => import("./setting-verge"),
  ];

  const onError = (err: any) => {
    useNotification({
      title: t("Error"),
      body: err.message || err.toString(),
      type: NotificationType.Error,
    });
  };

  const handleChange = async () => {
    if (!show) {
      const content =
        "Legacy Settings will be completely removed in a future update. They are retained here solely for debugging purposes. No fixes will be made for any issues encountered when using them.";

      await useMessage(content, {
        type: "warning",
        title: "Warning",
      });
    }

    setShow(!show);
  };

  return (
    <>
      <BaseCard label={t("Legacy Settings")}>
        <Box sx={{ pt: 1 }}>
          <LabelSwitch
            label={t("Enable Legacy Settings")}
            checked={show}
            onChange={() => handleChange()}
          />
        </Box>
      </BaseCard>

      {show &&
        legacyComponents.map((item, index) => {
          const AsyncComponent = lazy(item);

          return (
            <Paper key={index}>
              <Suspense>
                <AsyncComponent onError={onError} />
              </Suspense>
            </Paper>
          );
        })}
    </>
  );
};

export default SettingLegacy;
