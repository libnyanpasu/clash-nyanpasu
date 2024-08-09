import { useWhyDidYouUpdate } from "ahooks";
import { useAtomValue, useSetAtom } from "jotai";
import { mergeWith } from "lodash-es";
import { useEffect, useMemo } from "react";
import { defaultTheme } from "@/pages/_theme";
import { themeMode as themeModeAtom } from "@/store";
import { alpha, darken, lighten, Theme, useColorScheme } from "@mui/material";
import { useNyanpasu } from "@nyanpasu/interface";
import { cn, createMDYTheme } from "@nyanpasu/ui";
import { appWindow } from "@tauri-apps/api/window";

const applyRootStyleVar = (mode: "light" | "dark", theme: Theme) => {
  const root = document.documentElement;
  const palette = theme.colorSchemes[mode].palette;

  const isLightMode = mode !== "light";
  root.className = cn(mode === "dark" && "dark");
  const backgroundColor = isLightMode
    ? darken(palette.secondary.dark, 0.95)
    : lighten(palette.secondary.light, 0.95);

  const selectionColor = isLightMode ? "#d5d5d5" : "#f5f5f5";
  const scrollerColor = isLightMode ? "#54545480" : "#90939980";

  root.style.setProperty("--background-color", backgroundColor);
  root.style.setProperty("--selection-color", selectionColor);
  root.style.setProperty("--scroller-color", scrollerColor);
  root.style.setProperty("--primary-main", palette.primary.main);
  root.style.setProperty(
    "--background-color-alpha",
    alpha(palette.primary.main, 0.1),
  );
};

/**
 * custom theme
 */
export const useCustomTheme = () => {
  const { nyanpasuConfig } = useNyanpasu();
  const themeMode = useAtomValue(themeModeAtom);

  useWhyDidYouUpdate("useCustomTheme", { nyanpasuConfig, themeMode });

  const theme = useMemo(() => {
    const mergedTheme = createMDYTheme(
      mergeWith(
        {},
        defaultTheme,
        nyanpasuConfig?.theme_setting,
        (objValue, srcValue) => {
          return srcValue === undefined || srcValue === ""
            ? objValue
            : srcValue;
        },
      ),
    );

    applyRootStyleVar(themeMode, mergedTheme);

    return mergedTheme;
  }, [nyanpasuConfig?.theme_setting, themeMode]);

  return { theme };
};

export const ThemeModeProvider = () => {
  const { nyanpasuConfig } = useNyanpasu();

  const setThemeMode = useSetAtom(themeModeAtom);

  const { setMode } = useColorScheme();

  useEffect(() => {
    if (nyanpasuConfig?.theme_mode === "system") {
      appWindow.theme().then((m) => {
        if (m) {
          setThemeMode(m);
          setMode(m);
        }
      });

      const unlisten = appWindow.onThemeChanged((e) => {
        setThemeMode(e.payload);
        setMode(e.payload);
      });

      return () => {
        unlisten.then((fn) => fn());
      };
    }

    const chosenThemeMode = nyanpasuConfig?.theme_mode || "light";
    setThemeMode(chosenThemeMode);
    setMode(chosenThemeMode);
  }, [nyanpasuConfig?.theme_mode]);

  return null;
};
