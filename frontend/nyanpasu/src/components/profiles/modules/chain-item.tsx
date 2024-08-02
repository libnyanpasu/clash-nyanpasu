import { memo, useState, useTransition } from "react";
import { useTranslation } from "react-i18next";
import { Menu as MenuIcon } from "@mui/icons-material";
import { LoadingButton } from "@mui/lab";
import { alpha, ListItemButton, Menu, MenuItem, useTheme } from "@mui/material";
import { Profile, useClash } from "@nyanpasu/interface";
import { cleanDeepClickEvent } from "@nyanpasu/ui";

export const ChainItem = memo(function ChainItem({
  item,
  selected,
  onClick,
  onChainEdit,
}: {
  item: Profile.Item;
  selected?: boolean;
  onClick: () => Promise<void>;
  onChainEdit: () => void;
}) {
  const { t } = useTranslation();

  const { palette } = useTheme();

  const { deleteProfile, viewProfile } = useClash();

  const [isPending, startTransition] = useTransition();

  const handleClick = () => {
    startTransition(onClick);
  };

  const [anchorEl, setAnchorEl] = useState<null | HTMLElement>(null);

  const menuMapping = {
    Apply: () => handleClick(),
    "Edit Info": () => onChainEdit(),
    "Open File": () => viewProfile(item.uid),
    Delete: () => deleteProfile(item.uid),
  };

  const handleMenuClick = (func: () => void) => {
    setAnchorEl(null);
    func();
  };

  return (
    <>
      <ListItemButton
        className="!mb-2 !mt-2 !flex !justify-between gap-2"
        sx={{
          backgroundColor: selected
            ? alpha(palette.primary.main, 0.3)
            : alpha(palette.secondary.main, 0.1),
          borderRadius: 4,

          "&:hover": {
            backgroundColor: selected
              ? alpha(palette.primary.main, 0.5)
              : undefined,
          },
        }}
        onClick={handleClick}
        disabled={isPending}
      >
        <div className="truncate py-1">
          <span>{item.name}</span>
        </div>

        <LoadingButton
          size="small"
          color="primary"
          className="!size-8 !min-w-0"
          onClick={(e) => {
            cleanDeepClickEvent(e);
            setAnchorEl(e.currentTarget);
          }}
          loading={isPending}
        >
          <MenuIcon />
        </LoadingButton>
      </ListItemButton>

      <Menu
        anchorEl={anchorEl}
        open={Boolean(anchorEl)}
        onClose={() => setAnchorEl(null)}
      >
        {Object.entries(menuMapping).map(([key, func], index) => {
          return (
            <MenuItem
              key={index}
              onClick={(e) => {
                cleanDeepClickEvent(e);
                handleMenuClick(func);
              }}
            >
              {t(key)}
            </MenuItem>
          );
        })}
      </Menu>
    </>
  );
});

export default ChainItem;
