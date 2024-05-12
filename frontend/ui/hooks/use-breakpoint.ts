import { useTheme } from "@mui/material";
import { useEffect, useState } from "react";

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

  const [breakpoint, setBreakpoint] = useState({
    key: "sm",
    column: 1,
  });

  const getBreakpoint = (width: number) => {
    for (const [key, value] of Object.entries(breakpoints.values)) {
      if (value >= width) {
        setBreakpoint({ key, column: columnMapping[key] });
        return;
      }
    }

    setBreakpoint((p) => ({ ...p, column: columnMapping["default"] }));
  };

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
  }, []);

  return breakpoint;
};
