import { styled } from "@mui/material/styles";
import Switch, { SwitchProps } from "@mui/material/Switch";
import CircularProgress from "@mui/material/CircularProgress";
import "./mdy-switch.scss";

interface MDYSwitchProps extends SwitchProps {
  loading?: boolean;
}

const MDYSwitch = styled((props: MDYSwitchProps) => {
  const { loading = false, checked, disabled, ...nativeProps } = props;

  return (
    <div className="MDYSwitch-container">
      {loading && (
        <CircularProgress
          className={"MDYSwitch-CircularProgress " + (checked ? "checked" : "")}
          aria-labelledby={props.id}
          color="inherit"
          size={16}
        />
      )}
      <Switch
        {...nativeProps}
        checked={checked}
        disabled={loading || disabled}
      />
    </div>
  );
})(({ theme, checked, loading, disabled }) => ({
  height: "32px",
  padding: 0,
  margin: 0,
  borderRadius: 24,
  opacity: loading || disabled ? 0.5 : 1,
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
    padding: "6px",
  },
  "& .MuiSwitch-thumb": {
    boxShadow: "none",
    width: loading ? 24 : 16,
    height: loading ? 24 : 16,
    margin: loading ? -2 : 3,
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
}));

export default MDYSwitch;
