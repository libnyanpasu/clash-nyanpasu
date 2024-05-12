import { classNames } from "@/utils";
import { Bolt, Done } from "@mui/icons-material";
import {
  alpha,
  Button,
  CircularProgress,
  Tooltip,
  useTheme,
} from "@mui/material";
import { useDebounceFn, useLockFn } from "ahooks";
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

  const [mounted, setMounted] = useState(false);

  const { run: runMounted, cancel: cancelMounted } = useDebounceFn(
    () => setMounted(false),
    { wait: 1000 },
  );

  const handleClick = useLockFn(async () => {
    try {
      setLoading(true);
      setMounted(true);
      cancelMounted();

      await onClick();
    } finally {
      setLoading(false);
      runMounted();
    }
  });

  const isSuccess = mounted && !loading;

  return (
    <Tooltip title={t("Delay check")}>
      <Button
        className="size-16 backdrop-blur !rounded-2xl !fixed z-10 bottom-8 right-8"
        sx={{
          boxShadow: 8,
          backgroundColor: alpha(
            palette[isSuccess ? "success" : "primary"].main,
            isSuccess ? 0.7 : 0.3,
          ),

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
            "!transition-opacity",
            mounted ? "opacity-0" : "opacity-1",
          )}
        />

        {mounted && (
          <CircularProgress
            size={32}
            className={classNames(
              "transition-opacity",
              "absolute",
              loading ? "opacity-1" : "opacity-0",
            )}
          />
        )}

        <Done
          color="success"
          className={classNames(
            "!size-8",
            "absolute",
            "!transition-opacity",
            isSuccess ? "opacity-1" : "opacity-0",
          )}
        />
      </Button>
    </Tooltip>
  );
});
