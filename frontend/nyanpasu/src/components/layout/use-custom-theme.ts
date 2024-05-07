import { useEffect, useMemo } from "react";
import { alpha, Theme } from "@mui/material";
import { appWindow } from "@tauri-apps/api/window";
import { defaultTheme, defaultDarkTheme } from "@/pages/_theme";
import { useNyanpasu } from "@nyanpasu/interface";
import { createMDYTheme } from "@nyanpasu/ui";
import { useAtom } from "jotai";
import { themeMode } from "@/store";

const applyRootStyleVar = (mode: "light" | "dark", theme: Theme) => {
  const root = document.documentElement;

  root.style.setProperty(
    "--background-color",
    mode === "light" ? "#ffffff" : "#121212",
  );

  root.style.setProperty(
    "--selection-color",
    mode === "light" ? "#f5f5f5" : "#d5d5d5",
  );

  root.style.setProperty(
    "--scroller-color",
    mode === "light" ? "#90939980" : "#54545480",
  );

  root.style.setProperty("--primary-main", theme.palette.primary.main);

  root.style.setProperty(
    "--background-color-alpha",
    alpha(theme.palette.primary.main, 0.1),
  );
};

/**
 * custom theme
 */
export const useCustomTheme = () => {
  const { nyanpasuConfig } = useNyanpasu();

  const [mode, setMode] = useAtom(themeMode);

  useEffect(() => {
    if (nyanpasuConfig?.theme_mode === "system") {
      appWindow.theme().then((m) => m && setMode(m));

      const unlisten = appWindow.onThemeChanged((e) => setMode(e.payload));

      return () => {
        unlisten.then((fn) => fn());
      };
    }

    if (nyanpasuConfig?.theme_mode) {
      setMode(nyanpasuConfig?.theme_mode);
    } else {
      setMode("light");
    }
  }, [nyanpasuConfig?.theme_mode]);

  const theme = useMemo(() => {
    const dt = mode === "light" ? defaultTheme : defaultDarkTheme;

    const theme = createMDYTheme(
      {
        ...dt,
        ...nyanpasuConfig?.theme_setting,
      },
      mode,
    );

    applyRootStyleVar(mode, theme);

    return theme;
  }, [mode, nyanpasuConfig?.theme_setting]);

  return { theme };
};
