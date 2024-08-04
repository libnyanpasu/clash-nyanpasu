import { useMount } from "ahooks";
import dayjs from "dayjs";
import AppContainer from "@/components/app/app-container";
import LocalesProvider from "@/components/app/locales-provider";
import MutationProvider from "@/components/layout/mutation-provider";
import NoticeProvider from "@/components/layout/notice-provider";
import PageTransition from "@/components/layout/page-transition";
import SchemeProvider from "@/components/layout/scheme-provider";
import {
  ThemeModeProvider,
  useCustomTheme,
} from "@/components/layout/use-custom-theme";
import LogProvider from "@/components/logs/log-provider";
import { classNames } from "@/utils";
import { useTheme } from "@mui/material";
import { Experimental_CssVarsProvider as CssVarsProvider } from "@mui/material/styles";
import { useBreakpoint } from "@nyanpasu/ui";
import { emit } from "@tauri-apps/api/event";
import "dayjs/locale/ru";
import "dayjs/locale/zh-cn";
import relativeTime from "dayjs/plugin/relativeTime";
import { useAtom } from "jotai";
import { useEffect } from "react";
import { FallbackProps } from "react-error-boundary";
import { SWRConfig } from "swr";
import { atomIsDrawer } from "@/store";
import styles from "./_app.module.scss";

dayjs.extend(relativeTime);

export default function App() {
  const { theme } = useCustomTheme();

  const { column } = useBreakpoint();

  const [isDrawer, setIsDrawer] = useAtom(atomIsDrawer);

  useEffect(() => {
    setIsDrawer(Boolean(column === 1));
  }, [column]);

  useMount(() => {
    import("@tauri-apps/api/window")
      .then(({ appWindow }) => {
        appWindow.show();
        appWindow.unminimize();
        appWindow.setFocus();
      })
      .finally(() => emit("react_app_mounted"));
  });

  return (
    <SWRConfig
      value={{
        errorRetryCount: 5,
        revalidateOnMount: true,
        revalidateOnFocus: true,
        refreshInterval: 5000,
      }}
    >
      <CssVarsProvider theme={theme}>
        <ThemeModeProvider />
        <LogProvider />
        <LocalesProvider />
        <MutationProvider />
        <NoticeProvider />
        <SchemeProvider />

        <AppContainer isDrawer={isDrawer}>
          <PageTransition
            className={isDrawer ? "the-content-small" : "the-content"}
          />
        </AppContainer>
      </CssVarsProvider>
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
      <pre>{error}</pre>
    </div>
  );
};

export const Pending = () => <div>Loading from _app...</div>;
