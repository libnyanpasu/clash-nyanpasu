import { parseHotkey } from "@/utils/parse-hotkey";
import { DeleteRounded } from "@mui/icons-material";
import { alpha, IconButton, useTheme } from "@mui/material";
import Kbd from "@nyanpasu/ui/materialYou/components/kbd";
import clsx from "clsx";
import { CSSProperties, useRef, useState } from "react";
import styles from "./hotkey-input.module.scss";

export interface Props extends React.HTMLAttributes<HTMLInputElement> {
  isDuplicate?: boolean;
  value?: string[];
  onValueChange?: (value: string[]) => void;
  func: string;
  onBlurCb?: (e: React.FocusEvent<HTMLInputElement>, func: string) => void;
}

export default function HotkeyInput({
  isDuplicate = false,
  value,
  children,
  func,
  onValueChange,
  onBlurCb,
  // native
  className,
  ...rest
}: Props) {
  const theme = useTheme();

  const changeRef = useRef<string[]>([]);
  const [keys, setKeys] = useState(value || []);
  return (
    <div className="flex items-center gap-2">
      <div className={clsx("relative w-[165px] min-h-[36px]", styles.wrapper)}>
        <input
          className={clsx(
            "absolute top-0 left-0 w-full h-full z-[1] opacity-0",
            styles.input,
            className,
          )}
          onKeyUp={() => {
            const ret = changeRef.current.slice();
            if (ret.length) {
              onValueChange?.(ret);
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
          onBlur={(e) => {
            onBlurCb?.(e, func);
          }}
          {...rest}
        />
        <div
          className={clsx(
            "flex items-center flex-wrap w-full h-full min-h-[36px] box-border py-1 px-1 border border-solid rounded last:mr-0",
            styles.items,
          )}
          style={
            {
              "--border-color": isDuplicate
                ? theme.palette.error.main
                : alpha(theme.palette.text.secondary, 0.15),
              "--input-focus-border-color": alpha(
                theme.palette.primary.main,
                0.75,
              ),
              "--input-hover-border-color": `rgba(${theme.vars.palette.common.background} / 0.23)`,
            } as CSSProperties
          }
        >
          {keys.map((key) => (
            <Kbd className="scale-75" key={key}>
              {key}
            </Kbd>
          ))}
        </div>
      </div>

      <IconButton
        size="small"
        title="Delete"
        color="inherit"
        onClick={() => {
          onValueChange?.([]);
          setKeys([]);
          onBlurCb?.({} as any, func);
        }}
      >
        <DeleteRounded fontSize="inherit" />
      </IconButton>
    </div>
  );
}
