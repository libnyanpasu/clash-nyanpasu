import { classNames } from "@/utils";
import { Bolt } from "@mui/icons-material";
import {
  alpha,
  Button,
  CircularProgress,
  Tooltip,
  useTheme,
} from "@mui/material";
import { memo, useState } from "react";
import { useTranslation } from "react-i18next";

export const DelayButton = memo(function DelayButton({
  onClick,
}: {
  onClick: () => Promise<void>;
}) {
  const { t } = useTranslation();

  const { palette } = useTheme();

  const [loading, setLoading] = useState(false);

  const handleClick = async () => {
    try {
      setLoading(true);

      await onClick();
    } finally {
      setLoading(false);
    }
  };

  return (
    <Tooltip title={t("Delay check")}>
      <Button
        className="size-16 backdrop-blur !rounded-2xl !fixed z-10 bottom-16 right-16"
        sx={{
          boxShadow: 8,
          backgroundColor: alpha(palette.primary.main, 0.3),

          "&:hover": {
            backgroundColor: alpha(palette.primary.main, 0.45),
          },

          "&.MuiLoadingButton-loading": {
            backgroundColor: alpha(palette.primary.main, 0.15),
          },
        }}
        onClick={handleClick}
      >
        <Bolt
          className={classNames(
            "!size-8",
            "transition-opacity",
            loading ? "opacity-0" : "opacity-1",
          )}
        />

        <CircularProgress
          size={32}
          className={classNames(
            "transition-opacity",
            "absolute",
            loading ? "opacity-1" : "opacity-0",
          )}
        />
      </Button>
    </Tooltip>
  );
});
