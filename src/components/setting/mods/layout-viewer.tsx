import { BaseDialog, DialogRef } from "@/components/base";
import { pageTransitionVariants } from "@/components/layout/page-transition";
import { NotificationType, useNotification } from "@/hooks/use-notification";
import { useVerge } from "@/hooks/use-verge";
import { List, MenuItem, Select } from "@mui/material";
import { forwardRef, useImperativeHandle, useState } from "react";
import { useTranslation } from "react-i18next";
import { GuardState } from "./guard-state";
import { SettingItem } from "./setting-comp";
import MDYSwitch from "@/components/common/mdy-switch";

export const LayoutViewer = forwardRef<DialogRef>((props, ref) => {
  const { t } = useTranslation();
  const { verge, patchVerge, mutateVerge } = useVerge();

  const [open, setOpen] = useState(false);

  const [loading, setLoading] = useState({
    theme_blur: false,
    traffic_graph: false,
  });

  useImperativeHandle(ref, () => ({
    open: () => setOpen(true),
    close: () => setOpen(false),
  }));

  const onSwitchFormat = (_e: any, value: boolean) => value;
  const onError = (err: any) => {
    useNotification({
      title: t("Error"),
      body: err.message || err.toString(),
      type: NotificationType.Error,
    });
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
            onGuard={(e) => patchVerge({ theme_blur: e })}
            loading={loading["theme_blur"]}
          >
            <MDYSwitch edge="end" />
          </GuardState>
        </SettingItem>

        <SettingItem label={t("Traffic Graph")}>
          <GuardState
            value={verge?.traffic_graph ?? true}
            valueProps="checked"
            onCatch={onError}
            onFormat={onSwitchFormat}
            onGuard={(e) => patchVerge({ traffic_graph: e })}
            loading={loading["traffic_graph"]}
          >
            <MDYSwitch edge="end" />
          </GuardState>
        </SettingItem>

        <SettingItem label={t("Memory Usage")}>
          <GuardState
            value={verge?.enable_memory_usage ?? true}
            valueProps="checked"
            onCatch={onError}
            onFormat={onSwitchFormat}
            onGuard={(e) => patchVerge({ enable_memory_usage: e })}
          >
            <MDYSwitch edge="end" />
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
