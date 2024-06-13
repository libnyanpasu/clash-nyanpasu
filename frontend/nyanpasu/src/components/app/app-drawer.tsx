import { classNames } from "@/utils";
import { getRoutesWithIcon } from "@/utils/routes-utils";
import { Backdrop, IconButton, alpha, useTheme } from "@mui/material";
import AnimatedLogo from "../layout/animated-logo";
import RouteListItem from "./modules/route-list-item";
import { MenuOpen } from "@mui/icons-material";
import { useState } from "react";
import { AnimatePresence, motion } from "framer-motion";

export const AppDrawer = ({ isDrawer }: { isDrawer?: boolean }) => {
  const { palette } = useTheme();

  const routes = getRoutesWithIcon();

  const [open, setOpen] = useState(false);

  const Content = ({ className }: { className?: string }) => {
    return (
      <div
        className={classNames(
          "max-w-60",
          "min-w-28",
          "p-4",
          "pt-8",
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
        <div
          className="w-full h-full max-w-32 max-h-32 ml-auto mr-auto"
          data-windrag
        >
          <AnimatedLogo className="w-full h-full" data-windrag />
        </div>

        <div className="flex flex-col gap-2">
          {Object.entries(routes).map(([name, { path, icon }]) => {
            return (
              <RouteListItem key={name} name={name} path={path} icon={icon} />
            );
          })}
        </div>
      </div>
    );
  };

  const DrawerTitle = () => {
    return (
      <div
        className="flex items-center gap-2 fixed z-10 top-1.5 left-6"
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
    <Content />
  );
};

export default AppDrawer;
