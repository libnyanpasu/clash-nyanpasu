import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { NotificationType, useNotification } from "@/hooks/use-notification";
import { listen, UnlistenFn } from "@tauri-apps/api/event";

export const NoticeProvider = () => {
  const { t } = useTranslation();
  const unlistenFn = useRef<UnlistenFn>(null);
  useEffect(() => {
    listen<{
      set_config: { ok: string } | { err: string };
    }>("nyanpasu://notice-message", ({ payload }) => {
      if ("ok" in payload?.set_config) {
        useNotification({
          title: t("Success"),
          body: "Refresh Clash Config",
          type: NotificationType.Success,
        });
      } else if ("err" in payload?.set_config) {
        useNotification({
          title: t("Error"),
          body: payload.set_config.err,
          type: NotificationType.Error,
        });
      }
    })
      .then((unlisten) => {
        unlistenFn.current = unlisten;
      })
      .catch((e) => {
        useNotification({
          title: t("Error"),
          body: e.message,
          type: NotificationType.Error,
        });
      });
    return () => {
      unlistenFn.current?.();
    };
  }, []);

  return null;
};

export default NoticeProvider;
