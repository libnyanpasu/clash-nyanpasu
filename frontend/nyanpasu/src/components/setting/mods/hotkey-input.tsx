import Kbd from "@/components/common/kbd";
import { parseHotkey } from "@/utils/parse-hotkey";
import { DeleteRounded } from "@mui/icons-material";
import { Box, IconButton, alpha, styled } from "@mui/material";
import { FocusEvent, useRef, useState } from "react";

const KeyWrapper = styled("div")<{
  isDuplicate?: boolean;
}>(({ theme, isDuplicate }) => ({
  position: "relative",
  width: 165,
  minHeight: 36,

  "> input": {
    position: "absolute",
    top: 0,
    left: 0,
    width: "100%",
    height: "100%",
    zIndex: 1,
    opacity: 0,
  },
  "> input:focus + .list": {
    borderColor: alpha(theme.palette.primary.main, 0.75),
  },
  ".list": {
    display: "flex",
    alignItems: "center",
    flexWrap: "wrap",
    width: "100%",
    height: "100%",
    minHeight: 36,
    boxSizing: "border-box",
    padding: "2px 5px",
    border: "1px solid",
    borderRadius: 4,
    gap: 4,
    borderColor: isDuplicate
      ? theme.palette.error.main
      : alpha(theme.palette.text.secondary, 0.15),
    "&:last-child": {
      marginRight: 0,
    },
  },
}));

interface Props {
  func: string;
  isDuplicate: boolean;
  value: string[];
  onChange: (value: string[]) => void;
  onBlur?: (e: FocusEvent, func: string) => void;
}

export const HotkeyInput = (props: Props) => {
  const { value, onChange, func, isDuplicate } = props;

  const changeRef = useRef<string[]>([]);
  const [keys, setKeys] = useState(value);

  return (
    <Box sx={{ display: "flex", alignItems: "center" }}>
      <KeyWrapper isDuplicate={isDuplicate}>
        <input
          onKeyUp={() => {
            const ret = changeRef.current.slice();
            if (ret.length) {
              onChange(ret);
              changeRef.current = [];
            }
          }}
          onKeyDown={(e) => {
            const evt = e.nativeEvent;
            e.preventDefault();
            e.stopPropagation();

            const key = parseHotkey(evt.key);
            if (key === "UNIDENTIFIED") return;

            changeRef.current = [...new Set([...changeRef.current, key])];
            setKeys(changeRef.current);
          }}
          onBlur={(e) => props.onBlur && props.onBlur(e, func)}
        />

        <div className="list">
          {keys.map((key) => (
            <Kbd key={key}>{key}</Kbd>
          ))}
        </div>
      </KeyWrapper>

      <IconButton
        size="small"
        title="Delete"
        color="inherit"
        onClick={() => {
          onChange([]);
          setKeys([]);
          props.onBlur && props.onBlur({} as never, func);
        }}
      >
        <DeleteRounded fontSize="inherit" />
      </IconButton>
    </Box>
  );
};
