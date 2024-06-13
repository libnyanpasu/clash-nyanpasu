import { NotificationType, useNotification } from "@/hooks/use-notification";
import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";
import { useTranslation } from "react-i18next";

export const NoticeProvider = () => {
  const { t } = useTranslation();

  useEffect(() => {
    listen("verge://notice-message", ({ payload }) => {
      const [status, msg] = payload as [string, string];
      switch (status) {
        case "set_config::ok":
          useNotification({
            title: t("Success"),
            body: "Refresh Clash Config",
            type: NotificationType.Success,
          });
          break;

        case "set_config::error":
          useNotification({
            title: t("Error"),
            body: msg,
            type: NotificationType.Error,
          });
          break;

        default:
          break;
      }
    });
  }, []);

  return null;
};

export default NoticeProvider;
