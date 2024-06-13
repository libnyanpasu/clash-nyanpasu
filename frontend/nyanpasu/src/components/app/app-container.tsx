import getSystem from "@/utils/get-system";
import { LayoutControl } from "../layout/layout-control";
import AppDrawer from "./app-drawer";
import { ReactNode } from "react";
import styles from "./app-container.module.scss";
import { appWindow } from "@tauri-apps/api/window";
import Paper from "@mui/material/Paper";

const OS = getSystem();

export const AppContainer = ({
  children,
  isDrawer,
}: {
  children?: ReactNode;
  isDrawer?: boolean;
}) => {
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
      <AppDrawer isDrawer={isDrawer} data-windrag />

      <div className={styles.container}>
        {OS === "windows" && (
          <LayoutControl className="fixed right-6 top-1.5 !z-50" />
        )}

        <div className="h-9" data-windrag />

        {children}
      </div>
    </Paper>
  );
};

export default AppContainer;
