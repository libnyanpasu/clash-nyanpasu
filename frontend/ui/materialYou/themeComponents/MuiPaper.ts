import { Theme } from "@mui/material";
import { Components } from "@mui/material/styles/components";

export const MuiPaper: Components<Theme>["MuiPaper"] = {
  defaultProps: {
    sx: {
      borderRadius: 6,
      padding: 1,
      elevation: 2,
    },
  },
};
