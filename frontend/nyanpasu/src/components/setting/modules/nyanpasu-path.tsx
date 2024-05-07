import {
  alpha,
  ButtonBase,
  ButtonBaseProps,
  Paper,
  SxProps,
  Typography,
  useTheme,
} from "@mui/material";
import { memo, ReactNode } from "react";

export interface PaperButtonProps extends ButtonBaseProps {
  label: string;
  children?: ReactNode;
  sxPaper?: SxProps;
  sxButton?: SxProps;
}

export const PaperButton = memo(function PaperButton({
  label,
  children,
  sxPaper,
  sxButton,
  ...props
}: PaperButtonProps) {
  const { palette } = useTheme();

  return (
    <Paper
      elevation={0}
      sx={{
        borderRadius: 6,
        backgroundColor: alpha(palette.primary.main, 0.1),
        ...sxPaper,
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
          ...sxButton,
        }}
        {...props}
      >
        <Typography
          noWrap
          component="p"
          width="100%"
          sx={{
            fontWeight: 700,
            textOverflow: "ellipsis",
            overflow: "hidden",
          }}
        >
          {label}
        </Typography>

        {children}
      </ButtonBase>
    </Paper>
  );
});
