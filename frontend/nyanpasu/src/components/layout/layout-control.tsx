import { NotificationType, useNotification } from "@/hooks/use-notification";
import { save_window_size_state } from "@/services/cmds";
import {
  CloseRounded,
  CropSquareRounded,
  FilterNoneRounded,
  HorizontalRuleRounded,
} from "@mui/icons-material";
import { Button } from "@mui/material";
import { platform, type Platform } from "@tauri-apps/api/os";
import { appWindow } from "@tauri-apps/api/window";
import { debounce } from "lodash-es";
import { useEffect, useState } from "react";

export const LayoutControl = () => {
  const minWidth = 40;
  const [isMaximized, setIsMaximized] = useState(false);
  const [platfrom, setPlatform] = useState<Platform>("win32");
  const updateMaximized = async () => {
    try {
      const isMaximized = await appWindow.isMaximized();
      setIsMaximized(() => isMaximized);
    } catch (error) {
      useNotification({
        type: NotificationType.Error,
        title: "Error",
        body: typeof error === "string" ? error : (error as Error).message,
      });
    }
  };
  useEffect(() => {
    // Update the maximized state
    updateMaximized();
    // Get the platform
    platform().then((platform) => {
      setPlatform(() => platform);
    });
    // Add a resize handler to update the maximized state
    const resizeHandler = debounce(updateMaximized, 1000);
    window.addEventListener("resize", resizeHandler);
    return () => {
      window.removeEventListener("resize", resizeHandler);
    };
  }, []);
  return (
    <>
      <Button
        size="small"
        sx={{ minWidth, svg: { transform: "scale(0.9)" } }}
        onClick={() => appWindow.minimize()}
      >
        <HorizontalRuleRounded fontSize="small" />
      </Button>

      <Button
        size="small"
        sx={{ minWidth, svg: { transform: "scale(0.9)" } }}
        onClick={() => {
          setIsMaximized((isMaximized) => !isMaximized);
          appWindow.toggleMaximize();
        }}
      >
        {isMaximized ? (
          <FilterNoneRounded
            fontSize="small"
            style={{
              transform: "rotate(180deg) scale(0.8)",
            }}
          />
        ) : (
          <CropSquareRounded fontSize="small" />
        )}
      </Button>

      <Button
        size="small"
        sx={{ minWidth, svg: { transform: "scale(1.05)" } }}
        onClick={() => {
          if (platfrom === "win32") {
            save_window_size_state().finally(() => {
              appWindow.close();
            });
          } else {
            appWindow.close();
          }
        }}
      >
        <CloseRounded fontSize="small" />
      </Button>
    </>
  );
};
