import ListItem from "@mui/material/ListItem";
import ListItemButton from "@mui/material/ListItemButton";
import { Item } from "./clash-web";
import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";
import { alpha, useTheme } from "@mui/material/styles";
import { ClashCore, Core } from "@nyanpasu/interface";
import Clash from "@/assets/image/core/clash.png";
import ClashMeta from "@/assets/image/core/clash.meta.png";
import ClashRs from "@/assets/image/core/clash-rs.png";
import FiberManualRecord from "@mui/icons-material/FiberManualRecord";
import Update from "@mui/icons-material/Update";
import IconButton from "@mui/material/IconButton";
import Tooltip from "@mui/material/Tooltip";

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
}: ClashCoreItemProps) => {
  const { palette } = useTheme();

  const newVersion = data.latest ? data.latest !== data.version : false;

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

            {newVersion && (
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
            )}
          </Box>
        </Item>
      </ListItemButton>
    </ListItem>
  );
};
