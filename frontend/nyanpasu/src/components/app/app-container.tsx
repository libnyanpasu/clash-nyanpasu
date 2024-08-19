import { Allotment } from "allotment";
import getSystem from "@/utils/get-system";
import { alpha, useTheme } from "@mui/material";
import Paper from "@mui/material/Paper";
import { appWindow } from "@tauri-apps/api/window";
import "allotment/dist/style.css";
import { ReactNode } from "react";
import { LayoutControl } from "../layout/layout-control";
import styles from "./app-container.module.scss";
import AppDrawer from "./app-drawer";
import DrawerContent from "./drawer-content";

const OS = getSystem();

export const AppContainer = ({
  children,
  isDrawer,
}: {
  children?: ReactNode;
  isDrawer?: boolean;
}) => {
  // TODO: move layout sidecar size to nyanpasu config file for better compatibility?
  // const onLayout = useDebounce(() => {}, {
  //   wait: 100,
  // });

  const { palette } = useTheme();

  return (
    <Paper
      square
      elevation={0}
      className={styles.layout}
      onPointerDown={(e: any) => {
        if (e.target?.dataset?.windrag) {
          appWindow.startDragging();
        }
      }}
      onContextMenu={(e) => {
        e.preventDefault();
      }}
    >
      {isDrawer && <AppDrawer data-windrag />}

      <Allotment separator proportionalLayout={false}>
        {!isDrawer && (
          <Allotment.Pane className="h-full" minSize={96} maxSize={260}>
            <DrawerContent data-windrag />
          </Allotment.Pane>
        )}

        <Allotment.Pane visible={true} className={styles.container}>
          {OS === "windows" && (
            <LayoutControl className="!z-top fixed right-6 top-1.5" />
          )}

          {OS === "macos" && (
            <div
              className="z-top fixed left-6 top-3 h-8 w-[4.5rem] rounded-full"
              style={{ backgroundColor: alpha(palette.primary.main, 0.1) }}
            />
          )}

          <div
            className={OS === "macos" ? "h-[2.75rem]" : "h-9"}
            data-windrag
          />

          {children}
        </Allotment.Pane>
      </Allotment>
    </Paper>
  );
};

export default AppContainer;
