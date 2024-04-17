import { Theme } from "@mui/material";
import { Components } from "@mui/material/styles/components";

export const MuiButtonGroup: Components<Theme>["MuiButtonGroup"] = {
  styleOverrides: {
    grouped: {
      fontWeight: 700,
    },
    firstButton: {
      borderTopLeftRadius: 48,
      borderBottomLeftRadius: 48,

      "&.MuiButton-sizeSmall": {
        paddingLeft: "14px",
      },

      "&.MuiButton-sizeMedium": {
        paddingLeft: "20px",
      },

      "&.MuiButton-sizeLarge": {
        paddingLeft: "26px",
      },
    },
    lastButton: {
      borderTopRightRadius: 48,
      borderBottomRightRadius: 48,

      "&.MuiButton-sizeSmall": {
        paddingRight: "14px",
      },

      "&.MuiButton-sizeMedium": {
        paddingRight: "20px",
      },

      "&.MuiButton-sizeLarge": {
        paddingRight: "26px",
      },
    },
  },
};
