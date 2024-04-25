import {
  ButtonBaseProps,
  Paper,
  alpha,
  ButtonBase,
  Typography,
  CircularProgress,
  useTheme,
} from "@mui/material";
import { memo } from "react";

export interface PaperSwitchBottonProps extends ButtonBaseProps {
  label: string;
  checked: boolean;
  loading?: boolean;
}

export const PaperSwitchBotton = memo(function PaperSwitchBotton({
  label,
  checked,
  loading,
  ...props
}: PaperSwitchBottonProps) {
  const { palette } = useTheme();

  return (
    <Paper
      elevation={0}
      sx={{
        borderRadius: 6,
        backgroundColor: checked
          ? alpha(palette.primary.main, 0.1)
          : palette.mode == "dark"
            ? palette.common.black
            : palette.common.white,
        cursor: loading ? "progress" : "none",
      }}
    >
      <ButtonBase
        sx={{
          borderRadius: 6,
          width: "100%",
          textAlign: "start",
          padding: 2,
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
        }}
        {...props}
      >
        <Typography sx={{ fontWeight: 700 }}>{label}</Typography>

        {loading === true && <CircularProgress color="inherit" size={24} />}
      </ButtonBase>
    </Paper>
  );
});
