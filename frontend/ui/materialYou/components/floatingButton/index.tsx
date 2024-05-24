import { alpha, Button, ButtonProps, useTheme } from "@mui/material";
import { ReactNode } from "react";

export interface FloatingButtonProps extends ButtonProps {
  children: ReactNode;
  className?: string;
}

export const FloatingButton = ({
  children,
  className,
  ...props
}: FloatingButtonProps) => {
  const { palette } = useTheme();

  return (
    <Button
      className={`size-16 backdrop-blur !rounded-2xl z-10 bottom-8 right-8 ${className}`}
      sx={{
        position: "fixed",
        boxShadow: 8,
        backgroundColor: alpha(palette.primary.main, 0.3),

        "&:hover": {
          backgroundColor: alpha(palette.primary.main, 0.45),
        },
      }}
      {...props}
    >
      {children}
    </Button>
  );
};
