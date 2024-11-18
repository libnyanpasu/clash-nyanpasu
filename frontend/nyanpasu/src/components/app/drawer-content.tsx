import { AnimatePresence, motion } from "framer-motion";
import type { RefObject } from "react";
import getSystem from "@/utils/get-system";
import { getRoutesWithIcon } from "@/utils/routes-utils";
import { Box } from "@mui/material";
import { cn } from "@nyanpasu/ui";
import AnimatedLogo from "../layout/animated-logo";
import RouteListItem from "./modules/route-list-item";

export const DrawerContent = ({
  className,
  onlyIcon,
  ref,
}: {
  className?: string;
  onlyIcon?: boolean;
  ref?: RefObject<HTMLDivElement>;
}) => {
  const routes = getRoutesWithIcon();

  return (
    <AnimatePresence initial={false} mode="sync">
      <div
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
        style={{
          backgroundColor: "var(--background-color-alpha)",
        }}
        ref={ref}
        data-tauri-drag-region
      >
        <div className="mx-2 flex items-center justify-center">
          <div className="h-full max-h-28 max-w-28" data-tauri-drag-region>
            <AnimatedLogo className="h-full w-full" data-tauri-drag-region />
          </div>

          <motion.div
            className="mt-1 flex-1 whitespace-pre-wrap text-lg font-bold"
            data-tauri-drag-region
            initial={false}
            animate={onlyIcon ? "hidden" : "show"}
            variants={{
              show: {
                opacity: 1,
                scale: 1,
                maxWidth: 999,
                marginLeft: 16,
              },
              hidden: {
                opacity: 0,
                scale: 0,
                maxWidth: 0,
                marginLeft: 0,
              },
            }}
          >
            {"Clash\nNyanpasu"}
          </motion.div>
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
      </div>
    </AnimatePresence>
  );
};

export default DrawerContent;
