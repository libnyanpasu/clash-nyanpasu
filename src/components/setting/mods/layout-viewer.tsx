import { BaseDialog, DialogRef } from "@/components/base";
import { pageTransitionVariants } from "@/components/layout/page-transition";
import { useNotification } from "@/hooks/use-notification";
import { useVerge } from "@/hooks/use-verge";
import { List, MenuItem, Select, Switch } from "@mui/material";
import { forwardRef, useImperativeHandle, useState } from "react";
import { useTranslation } from "react-i18next";
import { GuardState } from "./guard-state";
import { SettingItem } from "./setting-comp";

export const LayoutViewer = forwardRef<DialogRef>((props, ref) => {
  const { t } = useTranslation();
  const { verge, patchVerge, mutateVerge } = useVerge();

  const [open, setOpen] = useState(false);

  useImperativeHandle(ref, () => ({
    open: () => setOpen(true),
    close: () => setOpen(false),
  }));

  const onSwitchFormat = (_e: any, value: boolean) => value;
  const onError = (err: any) => {
    useNotification(t("Error"), err.message || err.toString());
  };
  const onChangeData = (patch: Partial<IVergeConfig>) => {
    mutateVerge({ ...verge, ...patch }, false);
  };

  return (
    <BaseDialog
      open={open}
      title={t("Layout Setting")}
      contentSx={{ width: 450 }}
      disableOk
      cancelBtn={t("Cancel")}
      onClose={() => setOpen(false)}
      onCancel={() => setOpen(false)}
    >
      <List>
        <SettingItem label={t("Theme Blur")}>
          <GuardState
            value={verge?.theme_blur ?? false}
            valueProps="checked"
            onCatch={onError}
            onFormat={onSwitchFormat}
            onChange={(e) => onChangeData({ theme_blur: e })}
            onGuard={(e) => patchVerge({ theme_blur: e })}
          >
            <Switch edge="end" />
          </GuardState>
        </SettingItem>

        <SettingItem label={t("Traffic Graph")}>
          <GuardState
            value={verge?.traffic_graph ?? true}
            valueProps="checked"
            onCatch={onError}
            onFormat={onSwitchFormat}
            onChange={(e) => onChangeData({ traffic_graph: e })}
            onGuard={(e) => patchVerge({ traffic_graph: e })}
          >
            <Switch edge="end" />
          </GuardState>
        </SettingItem>

        <SettingItem label={t("Memory Usage")}>
          <GuardState
            value={verge?.enable_memory_usage ?? true}
            valueProps="checked"
            onCatch={onError}
            onFormat={onSwitchFormat}
            onChange={(e) => onChangeData({ enable_memory_usage: e })}
            onGuard={(e) => patchVerge({ enable_memory_usage: e })}
          >
            <Switch edge="end" />
          </GuardState>
        </SettingItem>
        {/* TODO: 将 select 单独开一个 Modal 以符合 Material Design 的设计 */}
        <SettingItem label={t("Page Transition Animation")}>
          <Select
            value={verge?.page_transition_animation ?? "slide"}
            style={{ width: 100 }}
            onChange={(e) => {
              onChangeData({
                page_transition_animation: e.target
                  .value as keyof typeof pageTransitionVariants,
              });
              patchVerge({
                page_transition_animation: e.target
                  .value as keyof typeof pageTransitionVariants,
              });
            }}
          >
            <MenuItem value="slide">
              {t("Page Transition Animation Slide")}
            </MenuItem>
            <MenuItem value="blur">
              {t("Page Transition Animation Blur")}
            </MenuItem>
            <MenuItem value="transparent">
              {t("Page Transition Animation Transparent")}
            </MenuItem>
            <MenuItem value="none">
              {t("Page Transition Animation None")}
            </MenuItem>
          </Select>
        </SettingItem>
      </List>
    </BaseDialog>
  );
});

LayoutViewer.displayName = "LayoutViewer";
