import { BaseDialog, DialogRef } from "@/components/base";
import { NotificationType, useNotification } from "@/hooks/use-notification";
import { useVerge } from "@/hooks/use-verge";
import { Typography, styled } from "@mui/material";
import { useLatest, useLockFn } from "ahooks";
import {
  FocusEvent,
  forwardRef,
  useEffect,
  useImperativeHandle,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import { HotkeyInput } from "./hotkey-input";

const ItemWrapper = styled("div")`
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 8px;
`;

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

export const HotkeyViewer = forwardRef<DialogRef>((props, ref) => {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);

  const { verge, patchVerge } = useVerge();

  const [hotkeyMap, setHotkeyMap] = useState<Record<string, string[]>>({});
  const hotkeyMapRef = useLatest(hotkeyMap);

  useImperativeHandle(ref, () => ({
    open: () => {
      setOpen(true);

      const map = {} as typeof hotkeyMap;

      verge?.hotkeys?.forEach((text) => {
        const [func, key] = text.split(",").map((e) => e.trim());

        if (!func || !key) return;

        map[func] = key
          .split("+")
          .map((e) => e.trim())
          .map((k) => (k === "PLUS" ? "+" : k));
      });

      setHotkeyMap(map);
      setDuplicateItems([]);
    },
    close: () => setOpen(false),
  }));

  // 检查是否有快捷键重复
  const [duplicateItems, setDuplicateItems] = useState<string[]>([]);
  const isDuplicate = !!duplicateItems.length;
  const onBlur = (e: FocusEvent, func: string) => {
    console.log(func);
    const keys = Object.values(hotkeyMapRef.current).flat().filter(Boolean);
    const set = new Set(keys);
    if (keys.length !== set.size) {
      setDuplicateItems([...duplicateItems, func]);
    } else {
      setDuplicateItems(duplicateItems.filter((e) => e !== func));
    }
  };

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
      await patchVerge({ hotkeys });
    } catch (err: any) {
      useNotification({
        title: t("Error"),
        body: err.message || err.toString(),
        type: NotificationType.Error,
      });
    }
  });

  useEffect(() => {
    if (!duplicateItems.length && open) {
      saveState();
    }
  }, [hotkeyMap, duplicateItems, open]);

  const onSave = () => {
    saveState().then(() => {
      setOpen(false);
    });
  };

  return (
    <BaseDialog
      open={open}
      title={t("Hotkey Viewer")}
      contentSx={{ width: 450, maxHeight: 330 }}
      okBtn={t("Save")}
      okBtnDisabled={isDuplicate}
      cancelBtn={t("Cancel")}
      onClose={() => setOpen(false)}
      onCancel={() => setOpen(false)}
      onOk={onSave}
    >
      {HOTKEY_FUNC.map((func) => (
        <ItemWrapper key={func}>
          <Typography>{t(func)}</Typography>
          <HotkeyInput
            func={func}
            isDuplicate={duplicateItems.includes(func)}
            onBlur={onBlur}
            value={hotkeyMap[func] ?? []}
            onChange={(v) => {
              const map = { ...hotkeyMapRef.current, [func]: v };
              hotkeyMapRef.current = map;
              setHotkeyMap(map);
            }}
          />
        </ItemWrapper>
      ))}
    </BaseDialog>
  );
});

HotkeyViewer.displayName = "HotkeyViewer";
