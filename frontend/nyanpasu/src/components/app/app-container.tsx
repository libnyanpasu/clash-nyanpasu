import getSystem from "@/utils/get-system";
import { alpha, useTheme } from "@mui/material";
import Paper from "@mui/material/Paper";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import "allotment/dist/style.css";
import { useAtom, useAtomValue } from "jotai";
import { ReactNode, useEffect, useRef } from "react";
import { Panel, PanelGroup, PanelResizeHandle } from "react-resizable-panels";
import { useElementBreakpoints } from "@/hooks/use-element-breakpoints";
import { atomIsDrawerOnlyIcon } from "@/store";
import { languageQuirks } from "@/utils/language";
import { useNyanpasu } from "@nyanpasu/interface";
import { cn } from "@nyanpasu/ui";
import { LayoutControl } from "../layout/layout-control";
import styles from "./app-container.module.scss";
import AppDrawer from "./app-drawer";
import DrawerContent from "./drawer-content";

const appWindow = getCurrentWebviewWindow();

const OS = getSystem();

export const AppContainer = ({
  children,
  isDrawer,
}: {
  children?: ReactNode;
  isDrawer?: boolean;
}) => {
  const { palette } = useTheme();

  const [onlyIcon, setOnlyIcon] = useAtom(atomIsDrawerOnlyIcon);

  const { nyanpasuConfig } = useNyanpasu();

  const sideRef = useRef<HTMLDivElement>(null!);

  const minWidth = nyanpasuConfig?.language
    ? languageQuirks[nyanpasuConfig?.language].drawer.minWidth
    : 180;

  const sideBreakpoint = useElementBreakpoints(
    sideRef,
    {
      "only-icon": 96,
      "with-text": minWidth,
    },
    "with-text",
  );

  useEffect(() => {
    if (sideBreakpoint === "only-icon") {
      setOnlyIcon(true);
    } else {
      setOnlyIcon(false);
    }
  }, [setOnlyIcon, sideBreakpoint]);

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
      {isDrawer && <AppDrawer data-tauri-drag-region />}

      <PanelGroup direction="horizontal">
        {!isDrawer && (
          <>
            {/* <Panel className={cn("min-w-24 max-w-64")}> */}
            <Panel className="min-w-24 max-w-64">
              <DrawerContent
                ref={sideRef}
                data-tauri-drag-region
                onlyIcon={onlyIcon}
              />
            </Panel>

            <PanelResizeHandle
              onDragEnd={() => {
                console.log("drag end");
              }}
            />
          </>
        )}

        <Panel
          order={1}
          minSize={50}
          className={cn("w-full", styles.container)}
        >
          {OS === "windows" && (
            <LayoutControl className="!z-top fixed right-4 top-2" />
          )}

          {OS === "macos" && (
            <div
              className="z-top fixed left-4 top-3 h-8 w-[4.5rem] rounded-full"
              style={{ backgroundColor: alpha(palette.primary.main, 0.1) }}
            />
          )}

          <div
            className={OS === "macos" ? "h-[2.75rem]" : "h-9"}
            data-tauri-drag-region
          />

          {children}
        </Panel>
      </PanelGroup>
    </Paper>
  );
};

export default AppContainer;
