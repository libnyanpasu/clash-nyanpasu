import getSystem from "@/utils/get-system";
import Paper from "@mui/material/Paper";
import { appWindow } from "@tauri-apps/api/window";
import { ReactNode } from "react";
import { Panel, PanelGroup, PanelResizeHandle } from "react-resizable-panels";
import { LayoutControl } from "../layout/layout-control";
import styles from "./app-container.module.scss";
import AppDrawer from "./app-drawer";

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
      <PanelGroup autoSaveId="layout_sidebar" direction="horizontal">
        <AppDrawer isDrawer={isDrawer} data-windrag />

        {!isDrawer && <PanelResizeHandle className={styles["resize-bar"]} />}

        <Panel order={2} minSize={50}>
          <div className={styles.container}>
            {OS === "windows" && (
              <LayoutControl className="fixed right-6 top-1.5 !z-50" />
            )}

            <div className="h-9" data-windrag />

            {children}
          </div>
        </Panel>
      </PanelGroup>
    </Paper>
  );
};

export default AppContainer;
