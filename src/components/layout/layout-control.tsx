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
import { useEffect, useState } from "react";

export const LayoutControl = () => {
  const minWidth = 40;
  const [isMaximized, setIsMaximized] = useState(false);
  const [platfrom, setPlatform] = useState<Platform>("win32");
  useEffect(() => {
    appWindow.isMaximized().then((isMaximized) => {
      setIsMaximized(() => isMaximized);
    });
    platform().then((platform) => {
      setPlatform(() => platform);
    });
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
