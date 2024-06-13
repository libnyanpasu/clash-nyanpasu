import { useTheme } from "@mui/material";
import { Experimental_CssVarsProvider as CssVarsProvider } from "@mui/material/styles";
import { AnimatePresence } from "framer-motion";
import { classNames } from "@/utils";
import {
  ThemeModeProvider,
  useCustomTheme,
} from "@/components/layout/use-custom-theme";
import PageTransition from "@/components/layout/page-transition";
import LogProvider from "@/components/logs/log-provider";
import LocalesProvider from "@/components/app/locales-provider";
import AppContainer from "@/components/app/app-container";
import NoticeProvider from "@/components/layout/notice-provider";
import SchemeProvider from "@/components/layout/scheme-provider";
import { useBreakpoint } from "@nyanpasu/ui";
import { FallbackProps } from "react-error-boundary";
import { SWRConfig } from "swr";
import styles from "./_app.module.scss";

import "dayjs/locale/ru";
import "dayjs/locale/zh-cn";

import dayjs from "dayjs";
import relativeTime from "dayjs/plugin/relativeTime";
import { useMemo } from "react";

dayjs.extend(relativeTime);

export default function App() {
  const { theme } = useCustomTheme();

  const { column } = useBreakpoint();

  const isDrawer = useMemo(() => Boolean(column === 1), [column]);

  return (
    <SWRConfig value={{ errorRetryCount: 3 }}>
      <CssVarsProvider theme={theme}>
        <ThemeModeProvider />
        <LogProvider />
        <LocalesProvider />
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
