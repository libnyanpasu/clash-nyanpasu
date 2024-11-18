import { RefObject, useEffect, useState } from "react";

export const useElementBreakpoints = (
  element: RefObject<HTMLElement>,
  breakpoints: { [key: string]: number },
  defaultBreakpoint: string,
) => {
  const [breakpoint, setBreakpoint] = useState<string>(defaultBreakpoint);

  const sortedBreakpoints = Object.entries(breakpoints).sort(
    ([, valueA], [, valueB]) => valueB - valueA,
  );

  useEffect(() => {
    let observer: ResizeObserver | null = null;

    if (element.current) {
      observer = new ResizeObserver(() => {
        const { width } = element.current!.getBoundingClientRect();

        const matchingBreakpoint =
          sortedBreakpoints.find(([, value]) => width >= value)?.[0] ??
          defaultBreakpoint;

        setBreakpoint(matchingBreakpoint);
      });

      observer.observe(element.current);
    }

    return () => observer?.disconnect();
  }, [element, breakpoints, defaultBreakpoint, sortedBreakpoints]);

  return breakpoint;
};
