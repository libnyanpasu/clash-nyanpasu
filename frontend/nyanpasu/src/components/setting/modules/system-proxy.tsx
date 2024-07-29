import { memo, ReactNode } from "react";
import { alpha, CircularProgress, useTheme } from "@mui/material";
import { PaperButton, PaperButtonProps } from "./nyanpasu-path";

export interface PaperSwitchButtonProps extends PaperButtonProps {
  label: string;
  checked: boolean;
  loading?: boolean;
  children?: ReactNode;
}

export const PaperSwitchButton = memo(function PaperSwitchButton({
  label,
  checked,
  loading,
  children,
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
            : palette.grey[100],
        cursor: loading ? "progress" : "none",
      }}
      sxButton={{
        flexDirection: "column",
        alignItems: "start",
        gap: 0.5,
      }}
      {...props}
    >
      {loading === true && (
        <CircularProgress
          sx={{
            position: "absolute",
            bottom: "calc(50% - 12px)",
            right: 12,
          }}
          color="inherit"
          size={24}
        />
      )}

      {children}
    </PaperButton>
  );
});
