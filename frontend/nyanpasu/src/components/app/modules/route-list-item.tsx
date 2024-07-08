import { classNames } from "@/utils";
import { languageQuirks } from "@/utils/language";
import { SvgIconComponent } from "@mui/icons-material";
import { ListItemButton, ListItemIcon, alpha, useTheme } from "@mui/material";
import { useNyanpasu } from "@nyanpasu/interface";
import { createElement } from "react";
import { useTranslation } from "react-i18next";
import { useMatch, useNavigate } from "react-router-dom";

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
        onlyIcon ? "!rounded-3xl !size-16 !mx-auto" : "!pr-14 !rounded-full",
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
            "pt-1 pb-1 w-full text-nowrap",
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
