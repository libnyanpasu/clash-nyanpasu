import {
  FilledInputProps,
  TextField,
  TextFieldProps,
  alpha,
  useTheme,
} from "@mui/material";
import { useTranslation } from "react-i18next";

export const HeaderSearch = (props: TextFieldProps) => {
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
    <TextField
      autoComplete="off"
      spellCheck="false"
      hiddenLabel
      placeholder={t("Type to Filter")}
      variant="filled"
      className="!pb-0"
      sx={{ input: { py: 1, fontSize: 14 } }}
      InputProps={inputProps}
      {...props}
    />
  );
};

export default HeaderSearch;
