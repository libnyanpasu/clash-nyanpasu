import { proxyGroupSortAtom } from "@/store";
import { Button, Menu, MenuItem } from "@mui/material";
import { useAtom } from "jotai";
import { memo, useState } from "react";
import { useTranslation } from "react-i18next";

export const SortSelector = memo(function SortSelector() {
  const { t } = useTranslation();

  const [proxyGroupSort, setProxyGroupSort] = useAtom(proxyGroupSortAtom);

  type SortType = typeof proxyGroupSort;

  const [anchorEl, setAnchorEl] = useState<null | HTMLElement>(null);

  const handleClick = (sort: SortType) => {
    setAnchorEl(null);
    setProxyGroupSort(sort);
  };

  const tmaps: { [key: string]: string } = {
    default: "Sort by default",
    delay: "Sort by delay",
    name: "Sort by name",
  };

  return (
    <>
      <Button
        size="small"
        variant="outlined"
        sx={{ textTransform: "none" }}
        onClick={(e) => setAnchorEl(e.currentTarget)}
      >
        {t(tmaps[proxyGroupSort])}
      </Button>

      <Menu
        anchorEl={anchorEl}
        open={Boolean(anchorEl)}
        onClose={() => setAnchorEl(null)}
      >
        {Object.entries(tmaps).map(([key, value], index) => {
          return (
            <MenuItem key={index} onClick={() => handleClick(key as SortType)}>
              {t(value)}
            </MenuItem>
          );
        })}
      </Menu>
    </>
  );
});

export default SortSelector;
