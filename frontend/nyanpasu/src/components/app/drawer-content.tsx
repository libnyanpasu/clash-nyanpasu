import { useSize } from "ahooks";
import { useAtom } from "jotai";
import { useCallback, useEffect, useRef } from "react";
import { atomIsDrawerOnlyIcon } from "@/store";
import getSystem from "@/utils/get-system";
import { languageQuirks } from "@/utils/language";
import { getRoutesWithIcon } from "@/utils/routes-utils";
import { Box, SxProps, Theme } from "@mui/material";
import { useNyanpasu } from "@nyanpasu/interface";
import { cn } from "@nyanpasu/ui";
import AnimatedLogo from "../layout/animated-logo";
import RouteListItem from "./modules/route-list-item";

export const DrawerContent = ({
  className,
  sx,
}: {
  className?: string;
  sx?: SxProps<Theme>;
}) => {
  const [onlyIcon, setOnlyIcon] = useAtom(atomIsDrawerOnlyIcon);

  const { nyanpasuConfig } = useNyanpasu();

  const routes = getRoutesWithIcon();

  const contentRef = useRef<HTMLDivElement | null>(null);

  const size = useSize(contentRef);

  const handleResize = useCallback(
    (value?: number) => {
      if (value) {
        if (
          value <
          languageQuirks[nyanpasuConfig?.language ?? "en"].drawer.minWidth
        ) {
          setOnlyIcon(true);
        } else {
          setOnlyIcon(false);
        }
      } else {
        setOnlyIcon(false);
      }
    },
    [nyanpasuConfig?.language, setOnlyIcon],
  );

  useEffect(() => {
    handleResize(size?.width);
  }, [handleResize, size?.width]);

  return (
    <Box
      ref={contentRef}
      className={cn(
        "p-4",
        getSystem() === "macos" ? "pt-14" : "pt-8",
        "w-full",
        "h-full",
        "flex",
        "flex-col",
        "gap-4",
        className,
      )}
      sx={[
        {
          backgroundColor: "var(--background-color-alpha)",
        },
      ]}
      data-tauri-drag-region
    >
      <div className="mx-2 flex items-center justify-center gap-4">
        <div className="h-full max-h-28 max-w-28" data-tauri-drag-region>
          <AnimatedLogo className="h-full w-full" data-tauri-drag-region />
        </div>

        {!onlyIcon && (
          <div
            className="mr-1 mt-1 whitespace-pre-wrap text-lg font-bold"
            data-tauri-drag-region
          >
            {"Clash\nNyanpasu"}
          </div>
        )}
      </div>

      <div className="scrollbar-hidden flex flex-col gap-2 overflow-y-auto !overflow-x-hidden">
        {Object.entries(routes).map(([name, { path, icon }]) => {
          return (
            <RouteListItem
              key={name}
              name={name}
              path={path}
              icon={icon}
              onlyIcon={onlyIcon}
            />
          );
        })}
      </div>
    </Box>
  );
};

export default DrawerContent;
