import { SvgIconComponent } from "@mui/icons-material";
import { ListItemButton, ListItemIcon, alpha, useTheme } from "@mui/material";
import { createElement } from "react";
import { useTranslation } from "react-i18next";
import { useMatch, useNavigate } from "react-router-dom";

export const RouteListItem = ({
  name,
  path,
  icon,
}: {
  name: string;
  path: string;
  icon: SvgIconComponent;
}) => {
  const { t } = useTranslation();

  const { palette } = useTheme();

  const match = useMatch({ path: path, end: true });

  const navigate = useNavigate();

  return (
    <ListItemButton
      className="!pr-12 !rounded-full"
      sx={{
        backgroundColor: match ? alpha(palette.primary.main, 0.3) : undefined,

        "&:hover": {
          backgroundColor: match ? alpha(palette.primary.main, 0.5) : undefined,
        },
      }}
      onClick={() => navigate(path)}
    >
      <ListItemIcon>
        {createElement(icon, {
          sx: {
            fill: match ? palette.primary.main : undefined,
          },
        })}
      </ListItemIcon>

      <div
        className="pt-1 pb-1"
        style={{ color: match ? palette.primary.main : undefined }}
      >
        {t(`label_${name}`)}
      </div>
    </ListItemButton>
  );
};

export default RouteListItem;
