import createTheme from "@mui/material/styles/createTheme";
import {
  argbFromHex,
  hexFromArgb,
  themeFromSourceColor,
} from "@material/material-color-utilities";

interface ThemeSchema {
  primary_color: string;
  secondary_color: string;
  primary_text: string;
  secondary_text: string;
  info_color: string;
  error_color: string;
  warning_color: string;
  success_color: string;
  font_family?: string;
}

export const createMDYTheme = (
  themeSchema: ThemeSchema,
  mode: "light" | "dark",
) => {
  const materialColor = themeFromSourceColor(
    argbFromHex(themeSchema.primary_color),
  );

  return createTheme({
    palette: {
      mode,
      primary: {
        main: hexFromArgb(materialColor.schemes[mode].primary),
      },
      secondary: {
        main: hexFromArgb(materialColor.schemes[mode].secondary),
      },
      error: {
        main: hexFromArgb(materialColor.schemes[mode].error),
      },
      text: {
        primary: hexFromArgb(materialColor.schemes[mode].onPrimaryContainer),
        secondary: hexFromArgb(
          materialColor.schemes[mode].onSecondaryContainer,
        ),
      },
    },
    typography: {
      fontFamily: themeSchema?.font_family,
    },
  });
};
