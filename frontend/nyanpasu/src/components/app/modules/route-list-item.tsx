import { createElement } from "react";
import { useTranslation } from "react-i18next";
import { useMatch, useNavigate } from "react-router-dom";
import { classNames } from "@/utils";
import { languageQuirks } from "@/utils/language";
import { SvgIconComponent } from "@mui/icons-material";
import { alpha, ListItemButton, ListItemIcon, useTheme } from "@mui/material";
import { useNyanpasu } from "@nyanpasu/interface";

export const RouteListItem = ({
  name,
  path,
  icon,
  onlyIcon,
}: {
  name: string;
  path: string;
  icon: SvgIconComponent;
  onlyIcon?: boolean;
}) => {
  const { t } = useTranslation();

  const { palette } = useTheme();

  const match = useMatch({ path: path, end: true });

  const navigate = useNavigate();

  const { nyanpasuConfig } = useNyanpasu();

  return (
    <ListItemButton
      className={classNames(
        onlyIcon ? "!mx-auto !size-16 !rounded-3xl" : "!rounded-full !pr-14",
      )}
      sx={{
        backgroundColor: match
          ? alpha(palette.primary.main, 0.3)
          : alpha(palette.background.paper, 0.15),

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
          className: onlyIcon ? "!size-8" : undefined,
        })}
      </ListItemIcon>

      {!onlyIcon && (
        <div
          className={classNames(
            "w-full text-nowrap pb-1 pt-1",
            nyanpasuConfig?.language &&
              languageQuirks[nyanpasuConfig?.language].drawer.itemClassNames,
          )}
          style={{ color: match ? palette.primary.main : undefined }}
        >
          {t(`label_${name}`)}
        </div>
      )}
    </ListItemButton>
  );
};

export default RouteListItem;
