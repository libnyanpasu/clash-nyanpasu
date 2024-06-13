import { memo, useState } from "react";
import FeatureChip from "./feature-chip";
import { getColorForDelay } from "./utils";
import { classNames } from "@/utils";
import { CircularProgress } from "@mui/material";

export const DelayChip = memo(function DelayChip({
  delay,
  onClick,
}: {
  delay: number;
  onClick: () => Promise<void>;
}) {
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
    <FeatureChip
      sx={{
        ml: "auto",
        color: getColorForDelay(delay),
      }}
      label={
        <>
          <span
            className={classNames(
              "transition-opacity",
              loading ? "opacity-0" : "opacity-1",
            )}
          >
            {delay ? `${delay} ms` : "timeout"}
          </span>

          <CircularProgress
            size={12}
            className={classNames(
              "transition-opacity",
              "absolute",
              "animate-spin",
              "top-0",
              "bottom-0",
              "left-0",
              "right-0",
              "m-auto",
              loading ? "opacity-1" : "opacity-0",
            )}
          />
        </>
      }
      variant="filled"
      onClick={(e) => {
        e.preventDefault();
        e.stopPropagation();
        handleClick();
      }}
    />
  );
});

export default DelayChip;
