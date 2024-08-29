import { AnimatePresence, motion } from "framer-motion";
import { useState } from "react";
import { classNames } from "@/utils";
import getSystem from "@/utils/get-system";
import { MenuOpen } from "@mui/icons-material";
import {
  alpha,
  Backdrop,
  darken,
  IconButton,
  lighten,
  useTheme,
} from "@mui/material";
import { cn } from "@nyanpasu/ui";
import AnimatedLogo from "../layout/animated-logo";
import DrawerContent from "./drawer-content";

const OS = getSystem();

export const AppDrawer = () => {
  const { palette } = useTheme();

  const [open, setOpen] = useState(false);

  const DrawerTitle = () => {
    return (
      <div
        className={classNames(
          "fixed z-10 flex items-center gap-2",
          OS === "macos" ? "left-24 top-3" : "left-4 top-1.5",
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
          <AnimatedLogo className="h-full w-full" data-windrag />
        </div>

        <div className="text-lg" data-windrag>
          Clash Nyanpasu
        </div>
      </div>
    );
  };

  return (
    <>
      <DrawerTitle />

      <Backdrop
        className={cn("z-20", OS !== "linux" && "backdrop-blur-xl")}
        sx={{
          backgroundColor:
            OS === "linux"
              ? undefined
              : alpha(palette.primary[palette.mode], 0.1),
        }}
        open={open}
        onClick={() => setOpen(false)}
      >
        <AnimatePresence initial={false}>
          <div className="h-full w-full">
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
              <DrawerContent
                className="max-w-64"
                style={{
                  backgroundColor:
                    OS === "linux"
                      ? palette.mode === "light"
                        ? lighten(palette.primary[palette.mode], 0.9)
                        : darken(palette.primary[palette.mode], 0.7)
                      : undefined,
                }}
              />
            </motion.div>
          </div>
        </AnimatePresence>
      </Backdrop>
    </>
  );
};

export default AppDrawer;
