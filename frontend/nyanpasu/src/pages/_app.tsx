import { LayoutControl } from "@/components/layout/layout-control";
import { LayoutTraffic } from "@/components/layout/layout-traffic";
import { useCustomTheme } from "@/components/layout/use-custom-theme";
import { NotificationType, useNotification } from "@/hooks/use-notification";
import { useVerge } from "@/hooks/use-verge";
import { getAxios } from "@/services/api";
import getSystem from "@/utils/get-system";
import { List, Paper, ThemeProvider, alpha, useTheme } from "@mui/material";
import { emit, listen } from "@tauri-apps/api/event";
import { appWindow } from "@tauri-apps/api/window";
import dayjs from "dayjs";
import "dayjs/locale/ru";
import "dayjs/locale/zh-cn";
import relativeTime from "dayjs/plugin/relativeTime";
import { AnimatePresence } from "framer-motion";
import i18next from "i18next";
import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { SWRConfig, mutate } from "swr";
// import { routers } from "./_routers";
import { LayoutItem } from "@/components/layout/layout-item";
import PageTransition from "@/components/layout/page-transition";
import { useNavigate, type Path } from "@/router";
import { classNames } from "@/utils";

import AnimatedLogo from "@/components/layout/animated-logo";
import { FallbackProps } from "react-error-boundary";
import styles from "./_app.module.scss";

dayjs.extend(relativeTime);

const OS = getSystem();

export const routes = {
  proxies: "/proxies",
  profiles: "/profiles",
  connections: "/connections",
  rules: "/rules",
  logs: "/logs",
  settings: "/settings",
  providers: "/providers",
};

export default function App() {
  const { t } = useTranslation();

  const { theme } = useCustomTheme();

  const { verge } = useVerge();
  const { theme_blur, language } = verge || {};

  const navigate = useNavigate();
  // const location = useLocation();
  // const routes = useRoutes(routers);
  // if (!routes) return null;

  useEffect(() => {
    window.addEventListener("keydown", (e) => {
      // macOS有cmd+w
      if (e.key === "Escape" && OS !== "macos") {
        appWindow.close();
      }
    });

    listen("verge://refresh-clash-config", async () => {
      // the clash info may be updated
      await getAxios(true);
      mutate("getProxies");
      mutate("getVersion");
      mutate("getClashConfig");
      mutate("getProviders");
    });

    // update the verge config
    listen("verge://refresh-verge-config", () => mutate("getVergeConfig"));

    // 设置提示监听
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

    listen("verge://mutate-proxies", () => {
      mutate("getProxies");
      mutate("getProviders");
    });

    listen("scheme-request-received", (req) => {
      const message: string = req.payload as string;
      const url = new URL(message);
      if (url.pathname.endsWith("/")) url.pathname = url.pathname.slice(0, -1);
      if (url.pathname.startsWith("//")) url.pathname = url.pathname.slice(1);
      switch (url.pathname) {
        case "/subscribe-remote-profile":
          navigate("/profiles", {
            state: {
              subscribe: {
                url: url.searchParams.get("url"),
                name: url.searchParams.has("name")
                  ? decodeURIComponent(url.searchParams.get("name")!)
                  : undefined,
                desc: url.searchParams.has("desc")
                  ? decodeURIComponent(url.searchParams.get("desc")!)
                  : undefined,
              },
            },
          });
      }
    });

    setTimeout(() => {
      appWindow.show();
      appWindow.unminimize();
      appWindow.setFocus();
      emit("init-complete");
    }, 50);
  }, []);

  useEffect(() => {
    if (language) {
      dayjs.locale(language === "zh" ? "zh-cn" : language);
      i18next.changeLanguage(language);
    }
  }, [language]);

  return (
    <SWRConfig value={{ errorRetryCount: 3 }}>
      <ThemeProvider theme={theme}>
        <Paper
          square
          elevation={0}
          className={`${OS} layout`}
          onPointerDown={(e: any) => {
            if (e.target?.dataset?.windrag) appWindow.startDragging();
          }}
          onContextMenu={(e) => {
            // only prevent it on Windows
            const validList = ["input", "textarea"];
            const target = e.currentTarget;
            if (
              OS === "windows" &&
              !(
                validList.includes(target.tagName.toLowerCase()) ||
                target.isContentEditable
              )
            ) {
              e.preventDefault();
            }
          }}
          sx={[
            ({ palette }) => ({
              bgcolor: alpha(palette.background.paper, theme_blur ? 0.8 : 1),
            }),
          ]}
        >
          <div className="layout__left" data-windrag>
            <AnimatedLogo />
            <List className="the-menu">
              {Object.entries(routes).map(([name, to]) => (
                <LayoutItem key={name} to={to as Path}>
                  {t(`label_${name}`)}
                </LayoutItem>
              ))}
            </List>

            <div className="the-traffic" data-windrag>
              <LayoutTraffic />
            </div>
          </div>

          <div className="layout__right">
            {OS === "windows" && (
              <div className="the-bar">
                <LayoutControl />
              </div>
            )}

            <div className="drag-mask" data-windrag />

            <AnimatePresence mode="wait" initial={false}>
              {/* {React.cloneElement(routes, { key: location.pathname })} */}
              <PageTransition />
            </AnimatePresence>
          </div>
        </Paper>
      </ThemeProvider>
    </SWRConfig>
  );
}

export const Catch = ({ error }: FallbackProps) => {
  const theme = useTheme();
  return (
    <div
      className={classNames(
        styles.oops,
        theme.palette.mode === "dark" && styles.dark,
      )}
    >
      <h1>Oops!</h1>
      <p>Something went wrong... Caught at _app error boundary.</p>
      <pre>{error.message}</pre>
    </div>
  );
};

export const Pending = () => <div>Loading from _app...</div>;
