import useSWR from "swr";
import ClashRs from "@/assets/image/core/clash-rs.png";
import ClashMeta from "@/assets/image/core/clash.meta.png";
import Clash from "@/assets/image/core/clash.png";
import FiberManualRecord from "@mui/icons-material/FiberManualRecord";
import Update from "@mui/icons-material/Update";
import { CircularProgress, CircularProgressProps } from "@mui/material";
import Box from "@mui/material/Box";
import IconButton from "@mui/material/IconButton";
import ListItem from "@mui/material/ListItem";
import ListItemButton from "@mui/material/ListItemButton";
import { alpha, useTheme } from "@mui/material/styles";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";
import { ClashCore, Core, inspectUpdater } from "@nyanpasu/interface";
import { Item } from "./clash-web";

function CircularProgressWithLabel(
  props: CircularProgressProps & { value: number },
) {
  return (
    <Box sx={{ position: "relative", display: "inline-flex" }}>
      <CircularProgress variant="determinate" {...props} />
      <Box
        sx={{
          top: 0,
          left: 0,
          bottom: 0,
          right: 0,
          position: "absolute",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <Typography
          variant="caption"
          component="div"
          color="text.secondary"
        >{`${Math.round(props.value)}%`}</Typography>
      </Box>
    </Box>
  );
}

export const getImage = (core: ClashCore) => {
  switch (core) {
    case "mihomo":
    case "mihomo-alpha": {
      return ClashMeta;
    }

    case "clash-rs": {
      return ClashRs;
    }

    default: {
      return Clash;
    }
  }
};

export interface ClashCoreItemProps {
  selected: boolean;
  data: Core;
  updaterId?: number;
  onUpdaterStateChanged?: (
    state: "success" | "error",
    message?: string,
  ) => void;
  onClick: (core: ClashCore) => void;
  onUpdate: (core: ClashCore) => void;
}

/**
 * @example
 * <ClashCoreItem
    data={core}
    selected={selected}
    onClick={() => changeClashCore(item.core)}
  />
 *
 * `Design for Clash Core used.`
 *
 * @author keiko233 <i@elaina.moe>
 * @copyright LibNyanpasu org. 2024
 */
export const ClashCoreItem = ({
  selected,
  data,
  onClick,
  onUpdate,
  updaterId,
  onUpdaterStateChanged,
}: ClashCoreItemProps) => {
  const { palette } = useTheme();

  const newVersion = data.latest ? data.latest !== data.version : false;
  const updaterInfo = useSWR(
    updaterId ? `/inspectId?updaterId=${updaterId}` : null,
    () => inspectUpdater(updaterId!),
    {
      refreshInterval: 100,
      onSuccess: (data) => {
        if (data.state === "done") {
          onUpdaterStateChanged?.("success");
        } else if (typeof data.state === "object" && data.state.failed) {
          onUpdaterStateChanged?.("error", data.state.failed);
        }
      },
    },
  );
  return (
    <ListItem sx={{ pl: 0, pr: 0 }}>
      <ListItemButton
        sx={{
          padding: 0,
          borderRadius: "16px",

          "&.Mui-selected": {
            backgroundColor: alpha(palette.success.main, 0.2),
          },
        }}
        selected={selected}
        onClick={() => onClick(data.core)}
      >
        <Item elevation={0} sx={{ width: "100%" }}>
          <Box display="flex" alignItems="center" gap={2}>
            <img style={{ width: "64px" }} src={getImage(data.core)} />

            <Box>
              <Typography variant="subtitle1" fontWeight={700}>
                {data.name}

                {newVersion && (
                  <FiberManualRecord
                    sx={{ height: 10, fill: palette.success.main }}
                  />
                )}
              </Typography>

              <Typography>{data.version}</Typography>

              {newVersion && (
                <Typography variant="body2">
                  New Version: {data.latest}
                </Typography>
              )}
            </Box>

            {newVersion &&
              (updaterInfo.data?.state !== "done" ? (
                <Tooltip
                  title={`Current State: ${updaterInfo.data?.state}\n Speed: ${updaterInfo.data?.downloader.speed}`}
                >
                  <CircularProgressWithLabel
                    value={
                      updaterInfo.data
                        ? updaterInfo.data.downloader.downloaded /
                          updaterInfo.data.downloader.total
                        : 0
                    }
                  />
                </Tooltip>
              ) : (
                <Tooltip title="Update Core">
                  <IconButton
                    sx={{ marginLeft: "auto" }}
                    onClick={(e) => {
                      e.preventDefault();
                      e.stopPropagation();
                      onUpdate(data.core);
                    }}
                  >
                    <Update />
                  </IconButton>
                </Tooltip>
              ))}
          </Box>
        </Item>
      </ListItemButton>
    </ListItem>
  );
};
