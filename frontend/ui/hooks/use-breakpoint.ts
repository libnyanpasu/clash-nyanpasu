import { useTheme } from "@mui/material";
import { appWindow } from "@tauri-apps/api/window";
import { useSetState, useThrottleFn } from "ahooks";
import { useEffect, useCallback, useMemo } from "react";

export const useBreakpoint = (
  columnMapping: { [key: string]: number } = {
    sm: 1,
    md: 1,
    lg: 2,
    xl: 3,
    default: 4,
  },
) => {
  const { breakpoints } = useTheme();

  const [breakpoint, setBreakpoint] = useSetState({
    key: "sm",
    column: 1,
  });

  const breakpointsValues = useMemo(() => {
    return Object.entries(breakpoints.values);
  }, [breakpoints.values]);

  const getBreakpoint = useCallback(
    async (width: number) => {
      const isMinimized = await appWindow.isMinimized();

      if (isMinimized) {
        return;
      }

      for (const [key, value] of breakpointsValues) {
        if (value >= width) {
          if (key !== breakpoint.key) {
            setBreakpoint({
              key,
              column: columnMapping[key],
            });
          }
          return;
        }
      }

      if (breakpoint.key !== "default") {
        setBreakpoint({
          column: columnMapping["default"],
          key: "default",
        });
      }
    },
    [breakpointsValues, columnMapping, breakpoint.key, setBreakpoint],
  );

  const { run: triggerBreakpoint } = useThrottleFn(
    () => {
      const width = document.body.clientWidth;
      getBreakpoint(width);
    },
    {
      wait: 100,
    },
  );

  useEffect(() => {
    const observer = new ResizeObserver(triggerBreakpoint);

    observer.observe(document.body);

    return () => {
      observer.disconnect();
    };
  }, [triggerBreakpoint]);

  return breakpoint;
};
