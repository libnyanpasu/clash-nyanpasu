import { parseHotkey } from "@/utils/parse-hotkey";
import { DeleteRounded } from "@mui/icons-material";
import { alpha, IconButton, useTheme } from "@mui/material";
import type {} from "@mui/material/themeCssVarsAugmentation";
import clsx from "clsx";
import { CSSProperties, useRef, useState } from "react";
import { Kbd } from "@nyanpasu/ui";
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
      <div className={clsx("relative min-h-[36px] w-[165px]", styles.wrapper)}>
        <input
          className={clsx(
            "absolute left-0 top-0 z-[1] h-full w-full opacity-0",
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
            "box-border flex h-full min-h-[36px] w-full flex-wrap items-center rounded border border-solid px-1 py-1 last:mr-0",
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
