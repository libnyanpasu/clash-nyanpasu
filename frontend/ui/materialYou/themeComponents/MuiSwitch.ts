import { Theme } from "@mui/material";
import { Components } from "@mui/material/styles/components";
import { Palette } from "@mui/material/styles/createPalette";

export const MuiSwitch = (palette: Palette): Components<Theme>["MuiSwitch"] => {
  const isDark = palette.mode === "dark";

  return {
    styleOverrides: {
      root: {
        padding: 0,
        margin: 0,

        "& .Mui-checked": {
          "& .MuiSwitch-thumb": {
            color: palette.grey.A100,
          },
        },

        "&:has(.Mui-checked) .MuiSwitch-track::before": {
          opacity: 0,
        },

        "&:has(.Mui-disabled) .MuiSwitch-track": {
          opacity: "0.5 !important",
          cursor: "not-allowed",
        },
      },
      track: {
        borderRadius: "48px",
        backgroundColor: isDark ? palette.grey.A700 : palette.grey.A200,
        opacity: `${isDark ? 0.7 : 1} !important`,

        "&::before": {
          content: '""',
          border: `solid 2px ${palette.grey.A700}`,
          width: "100%",
          height: "100%",
          opacity: 1,
          position: "absolute",
          borderRadius: "inherit",
          boxSizing: "border-box",
          transitionProperty: "opacity, background-color",
          transitionTimingFunction: "linear",
          transitionDuration: "100ms",
        },
      },
      thumb: {
        boxShadow: "none",
        color: isDark ? palette.grey.A200 : palette.grey.A700,
      },
    },
    variants: [
      {
        props: {
          size: "medium",
        },
        style: {
          height: 32,

          "& .MuiSwitch-switchBase": {
            padding: "6px",
          },

          "& .MuiSwitch-thumb": {
            width: 14,
            height: 14,
            margin: 3,
          },

          "& .Mui-checked": {
            "&.MuiSwitch-switchBase": {
              marginLeft: "6px",
            },

            "& .MuiSwitch-thumb": {
              width: 24,
              height: 24,
              margin: -2,
            },
          },
        },
      },
      {
        props: {
          size: "small",
        },
        style: {
          height: 24,

          "& .MuiSwitch-switchBase": {
            padding: "3px",
          },

          "& .MuiSwitch-thumb": {
            width: 12,
            height: 12,
            margin: 3,
          },

          "& .Mui-checked": {
            "&.MuiSwitch-switchBase": {
              marginLeft: "1px",
            },

            "& .MuiSwitch-thumb": {
              width: 17,
              height: 17,
              margin: 0,
            },
          },
        },
      },
    ],
  };
};
