import { alpha, CircularProgress, useTheme } from "@mui/material";
import { memo } from "react";
import { PaperButton, PaperButtonProps } from "./nyanpasu-path";

export interface PaperSwitchButtonProps extends PaperButtonProps {
  label: string;
  checked: boolean;
  loading?: boolean;
}

export const PaperSwitchButton = memo(function PaperSwitchButton({
  label,
  checked,
  loading,
  ...props
}: PaperSwitchButtonProps) {
  const { palette } = useTheme();

  return (
    <PaperButton
      label={label}
      sxPaper={{
        backgroundColor: checked
          ? alpha(palette.primary.main, 0.1)
          : palette.mode == "dark"
            ? palette.common.black
            : palette.common.white,
        cursor: loading ? "progress" : "none",
      }}
      {...props}
    >
      {loading === true && <CircularProgress color="inherit" size={24} />}
    </PaperButton>
  );
});
