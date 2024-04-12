import { styled } from "@mui/material/styles";
const Kbd = styled("kbd")(({ theme }) => ({
  backgroundColor:
    theme.palette.mode === "dark" ? "rgb(255 255 255 / 0.06)" : "#edf2f7;",
  borderColor:
    theme.palette.mode === "dark" ? "rgb(255 255 255 / 0.16)" : "#e2e8f0",
  paddingRight: "0.4em",
  paddingLeft: "0.4em",
  fontFamily: "SFMono-Regular, Menlo, Monaco, Consolas, monospace",
  // font-size: 1em,
  fontSize: "0.8em",
  fontWeight: "bold",
  lineHeight: "normal",
  whiteSpace: "nowrap",
  borderWidth: "1px",
  borderBottomWidth: "3px",
  borderRadius: "0.375rem",
  borderStyle: "solid",
}));

export default Kbd;
