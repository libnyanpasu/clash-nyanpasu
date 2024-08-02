import { useLockFn, useMemoizedFn } from "ahooks";
import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { notification, NotificationType } from "@/utils/notification";
import { Typography } from "@mui/material";
import { useNyanpasu } from "@nyanpasu/interface";
import { BaseDialog, BaseDialogProps } from "@nyanpasu/ui";
import HotkeyInput from "./hotkey-input";

export type HotkeyDialogProps = Omit<BaseDialogProps, "title">;

const HOTKEY_FUNC = [
  "open_or_close_dashboard",
  "clash_mode_rule",
  "clash_mode_global",
  "clash_mode_direct",
  "clash_mode_script",
  "toggle_system_proxy",
  "enable_system_proxy",
  "disable_system_proxy",
  "toggle_tun_mode",
  "enable_tun_mode",
  "disable_tun_mode",
];

export default function HotkeyDialog({
  open,
  onClose,
  children,
  ...rest
}: HotkeyDialogProps) {
  const { t } = useTranslation();

  const { nyanpasuConfig, setNyanpasuConfig } = useNyanpasu();

  const [hotkeyMap, setHotkeyMap] = useState<Record<string, string[]>>({});
  const hotkeyMapRef = useRef<Record<string, string[]>>({});
  // 检查是否有快捷键重复
  const [duplicateItems, setDuplicateItems] = useState<string[]>([]);
  useEffect(() => {
    if (open) {
      const map = {} as typeof hotkeyMap;
      nyanpasuConfig?.hotkeys?.forEach((text) => {
        const [func, key] = text.split(",").map((i) => i.trim());
        if (!func || !key) return;
        map[func] = key
          .split("+")
          .map((e) => e.trim())
          .map((k) => (k === "PLUS" ? "+" : k));
      });
      setHotkeyMap(map);
      setDuplicateItems([]);
    }
  }, [open]);
  const isDuplicated = useMemo(() => !!duplicateItems.length, [duplicateItems]);

  const onBlurCb = useMemoizedFn(
    (e: React.FocusEvent<HTMLInputElement>, func: string) => {
      console.log(func);
      const keys = Object.values(hotkeyMapRef.current).flat().filter(Boolean);
      const set = new Set(keys);
      if (keys.length !== set.size) {
        setDuplicateItems([...duplicateItems, func]);
      } else {
        setDuplicateItems(duplicateItems.filter((e) => e !== func));
      }
    },
  );

  const saveState = useLockFn(async () => {
    const hotkeys = Object.entries(hotkeyMap)
      .map(([func, keys]) => {
        if (!func || !keys?.length) return "";

        const key = keys
          .map((k) => k.trim())
          .filter(Boolean)
          .map((k) => (k === "+" ? "PLUS" : k))
          .join("+");

        if (!key) return "";
        return `${func},${key}`;
      })
      .filter(Boolean);

    try {
      await setNyanpasuConfig({ hotkeys });
    } catch (err: any) {
      notification({
        title: t("Error"),
        body: err.message || err.toString(),
        type: NotificationType.Error,
      });
    }
  });

  // 自动保存
  useEffect(() => {
    if (!isDuplicated && open) {
      saveState();
    }
  }, [hotkeyMap, isDuplicated, open]);

  const onSave = () => {
    saveState().then(() => {
      onClose?.();
    });
  };

  return (
    <BaseDialog
      title={t("Hotkeys Setting")}
      open={open}
      onClose={onClose}
      {...rest}
    >
      {children}
      <div className="grid-1 grid gap-3">
        {HOTKEY_FUNC.map((func) => (
          <div className="flex items-center justify-between px-2" key={func}>
            <Typography>{t(func)}</Typography>
            <HotkeyInput
              func={func}
              isDuplicate={duplicateItems.includes(func)}
              onBlurCb={onBlurCb}
              value={hotkeyMap[func] ?? []}
              onValueChange={(v) => {
                const map = { ...hotkeyMapRef.current, [func]: v };
                hotkeyMapRef.current = map;
                setHotkeyMap(map);
              }}
            />
          </div>
        ))}
      </div>
    </BaseDialog>
  );
}
