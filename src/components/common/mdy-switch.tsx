import { styled } from "@mui/material/styles";
import Switch, { SwitchProps } from "@mui/material/Switch";

const MDYSwitch = styled((props: SwitchProps) => <Switch {...props} />)(
  ({ theme, checked }) => ({
    height: "32px",
    padding: 0,
    margin: 0,
    borderRadius: 24,
    "& .MuiSwitch-track": {
      borderRadius: 24,
      opacity: checked
        ? "1 !important"
        : theme.palette.mode === "dark"
          ? "0.3 !important"
          : "0.7 !important",
      backgroundColor: checked
        ? theme.palette.primary.main
        : theme.palette.mode === "dark"
          ? theme.palette.grey.A700
          : theme.palette.grey.A200,
      "&::before": {
        content: '""',
        border: `solid 2px ${theme.palette.grey.A700}`,
        width: "100%",
        height: "100%",
        opacity: checked ? 0 : 1,
        position: "absolute",
        borderRadius: "inherit",
        boxSizing: "border-box",
        transitionProperty: "opacity, background-color",
        transitionTimingFunction: "linear",
        transitionDuration: "67ms",
      },
    },
    "& .MuiSwitch-switchBase": {
      padding: "6px 9px 6px 6px",
    },
    "& .MuiSwitch-thumb": {
      boxShadow: "none",
      width: 16,
      height: 16,
      margin: 3,
      color: checked
        ? theme.palette.getContrastText(theme.palette.primary.main)
        : theme.palette.mode === "dark"
          ? theme.palette.grey.A200
          : theme.palette.grey.A700,
      opacity: checked ? 1 : 0.7,
    },
    "& .Mui-checked": {
      "&.MuiSwitch-switchBase": {
        padding: "6px 9px 6px 12px",
      },
      "& .MuiSwitch-thumb": {
        width: 24,
        height: 24,
        margin: -2,
      },
    },
  }),
);

export default MDYSwitch;
