import { useTheme } from "@mui/material";
import { Clash } from "@nyanpasu/interface";

export type History = Clash.Proxy["history"];

export const filterDelay = (history?: History): number => {
  if (!history || history.length == 0) {
    return 0;
  } else {
    return history[history.length - 1].delay;
  }
};

export const getColorForDelay = (delay: number): string => {
  const { palette } = useTheme();

  const delayColorMapping: { [key: string]: string } = {
    "0": palette.error.main,
    "1": palette.text.secondary,
    "100": palette.success.main,
    "500": palette.warning.main,
    "10000": palette.error.main,
  };

  let color: string = palette.text.secondary;

  for (const key in delayColorMapping) {
    if (delay <= parseInt(key)) {
      color = delayColorMapping[key];
      break;
    }
  }

  return color;
};
