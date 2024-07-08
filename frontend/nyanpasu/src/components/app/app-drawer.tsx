import { getRoutesWithIcon } from "@/utils/routes-utils";
import { MenuOpen } from "@mui/icons-material";
import { Backdrop, IconButton, alpha, useTheme } from "@mui/material";
import clsx from "clsx";
import { AnimatePresence, motion } from "framer-motion";
import { useCallback, useState } from "react";
import { Panel } from "react-resizable-panels";
import AnimatedLogo from "../layout/animated-logo";
import RouteListItem from "./modules/route-list-item";
import { classNames } from "@/utils";
import getSystem from "@/utils/get-system";
import { useNyanpasu } from "@nyanpasu/interface";
import { languageQuirks } from "@/utils/language";

export const AppDrawer = ({ isDrawer }: { isDrawer?: boolean }) => {
  const { palette } = useTheme();

  const routes = getRoutesWithIcon();

  const [open, setOpen] = useState(false);

  const { nyanpasuConfig } = useNyanpasu();

  const [onlyIcon, setOnlyIcon] = useState(false);

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
    [nyanpasuConfig?.language],
  );

  const Content = ({ className }: { className?: string }) => {
    return (
      <div
        className={clsx(
          isDrawer ? ["max-w-60", "min-w-28"] : "w-full",
          "p-4",
          getSystem() === "macos" ? "pt-14" : "pt-8",
          "h-full",
          "flex",
          "flex-col",
          "gap-4",
          className,
        )}
        style={{
          backgroundColor: "var(--background-color-alpha)",
        }}
        data-windrag
      >
        <div className="flex items-center justify-center gap-4 mx-2">
          <div className=" h-full max-w-28 max-h-28" data-windrag>
            <AnimatedLogo className="w-full h-full" data-windrag />
          </div>

          {(isDrawer || !onlyIcon) && (
            <div
              className={classNames(
                "text-lg font-bold mt-1 whitespace-pre-wrap",
                isDrawer && "mr-1",
              )}
              data-windrag
            >
              {"Clash\nNyanpasu"}
            </div>
          )}
        </div>

        <div className="flex flex-col gap-2 overflow-y-auto scrollbar-hidden !overflow-x-hidden">
          {Object.entries(routes).map(([name, { path, icon }]) => {
            return (
              <RouteListItem
                key={name}
                name={name}
                path={path}
                icon={icon}
                onlyIcon={!isDrawer && onlyIcon}
              />
            );
          })}
        </div>
      </div>
    );
  };

  const DrawerTitle = () => {
    return (
      <div
        className={classNames(
          "flex items-center gap-2 fixed z-10",
          getSystem() === "macos" ? "left-[6.5rem] top-3" : "left-6 top-1.5",
        )}
        data-windrag
      >
        <IconButton
          className="!size-8 !min-w-0"
          sx={{
            backgroundColor: alpha(palette.primary.main, 0.1),
            svg: { transform: "scale(0.9)" },
          }}
          onClick={() => setOpen(true)}
        >
          <MenuOpen />
        </IconButton>

        <div className="size-5" data-windrag>
          <AnimatedLogo className="w-full h-full" data-windrag />
        </div>

        <div className="text-lg" data-windrag>
          Clash Nyanpasu
        </div>
      </div>
    );
  };

  return isDrawer ? (
    <>
      <DrawerTitle />

      <Backdrop
        className="z-20 backdrop-blur-xl"
        sx={{
          backgroundColor: alpha(palette.primary[palette.mode], 0.1),
        }}
        open={open}
        onClick={() => setOpen(false)}
      >
        <AnimatePresence initial={false}>
          <div className="w-full h-full">
            <motion.div
              className="h-full"
              animate={open ? "open" : "closed"}
              variants={{
                open: {
                  x: 0,
                },
                closed: {
                  x: -240,
                },
              }}
              transition={{
                type: "tween",
              }}
            >
              <Content />
            </motion.div>
          </div>
        </AnimatePresence>
      </Backdrop>
    </>
  ) : (
    <Panel
      id="sidebar"
      defaultSize={
        languageQuirks[nyanpasuConfig?.language ?? "en"].drawer.minWidth
      }
      order={1}
      minSize={languageQuirks[nyanpasuConfig?.language ?? "en"].drawer.minWidth}
      collapsedSize={11}
      maxSize={36}
      onResize={handleResize}
      collapsible
    >
      <Content />
    </Panel>
  );
};

export default AppDrawer;
