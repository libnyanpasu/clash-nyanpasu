import { FilledInputProps, Portal, alpha, useTheme } from "@mui/material";
import { GridToolbarQuickFilter } from "@mui/x-data-grid";
import { Fragment } from "react";
import { useTranslation } from "react-i18next";

export const HeaderSearch = () => {
  const { t } = useTranslation();

  const { palette } = useTheme();

  const inputProps: Partial<FilledInputProps> = {
    sx: {
      borderRadius: 7,
      backgroundColor: alpha(palette.primary.main, 0.1),

      "&::before": {
        display: "none",
      },

      "&::after": {
        display: "none",
      },
    },
  };

  return (
    <Fragment>
      <Portal container={() => document.getElementById("filter-panel")}>
        <GridToolbarQuickFilter
          autoComplete="off"
          spellCheck="false"
          hiddenLabel
          placeholder={t("Type to Filter")}
          variant="filled"
          className="!pb-0"
          sx={{ input: { py: 1, fontSize: 14 } }}
          InputProps={inputProps}
        />
      </Portal>
    </Fragment>
  );
};

export default HeaderSearch;
