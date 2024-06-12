import { useTheme } from "@mui/material";
import { useSetState } from "ahooks";
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
    (width: number) => {
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

  useEffect(() => {
    const observer = new ResizeObserver((entries) => {
      if (!Array.isArray(entries) || !entries.length) return;

      const { width } = entries[0].contentRect;

      getBreakpoint(width);
    });

    observer.observe(document.body);

    return () => {
      observer.disconnect();
    };
  }, [getBreakpoint]);

  return breakpoint;
};
