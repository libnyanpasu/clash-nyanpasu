import { BaseDialog, DialogRef } from "@/components/base";
import { NotificationType, useNotification } from "@/hooks/use-notification";
import { useVerge } from "@/hooks/use-verge";
import { defaultDarkTheme, defaultTheme } from "@/pages/_theme";
import {
  List,
  ListItem,
  ListItemText,
  TextField,
  styled,
  useTheme,
} from "@mui/material";
import { useLockFn } from "ahooks";
import { forwardRef, useImperativeHandle, useState } from "react";
import { useTranslation } from "react-i18next";
import { MuiColorInput } from "mui-color-input";
import React from "react";

export const ThemeViewer = forwardRef<DialogRef>((props, ref) => {
  const { t } = useTranslation();

  const [open, setOpen] = useState(false);
  const { verge, patchVerge } = useVerge();
  const { theme_setting } = verge ?? {};
  const [theme, setTheme] = useState(theme_setting || {});

  useImperativeHandle(ref, () => ({
    open: () => {
      setOpen(true);
      setTheme({ ...theme_setting } || {});
    },
    close: () => setOpen(false),
  }));

  const textProps = {
    size: "small",
    autoComplete: "off",
    sx: { width: 135 },
  } as const;

  const handleChange = (field: keyof typeof theme) => (e: any) => {
    setTheme((t) => ({ ...t, [field]: e.target.value }));
  };

  const onSave = useLockFn(async () => {
    try {
      const msgs = (Object.keys(theme) as Array<keyof typeof theme>).reduce(
        (acc, cur) => {
          if (theme[cur] === "") {
            return acc;
          }
          // theme.page_transition_duration should be string here
          if (cur === "page_transition_duration") {
            acc[cur] = parseFloat(
              theme.page_transition_duration as unknown as string,
            );
          } else {
            acc[cur] = theme[cur];
          }
          return acc;
        },
        {} as Exclude<IVergeConfig["theme_setting"], undefined>,
      );
      await patchVerge({ theme_setting: msgs });
      setOpen(false);
    } catch (err: any) {
      useNotification({
        title: t("Error"),
        body: err.message || err.toString(),
        type: NotificationType.Error,
      });
    }
  });

  // default theme
  const { palette } = useTheme();

  const dt = palette.mode === "light" ? defaultTheme : defaultDarkTheme;

  type ThemeKey = keyof typeof theme & keyof typeof defaultTheme;

  const renderItem = (label: string, key: ThemeKey) => {
    const [color, setColor] = React.useState(theme[key] || dt[key]);

    const onChange = (color: string) => {
      if (!color) {
        color = dt[key];
      }

      setColor(color);
      setTheme((t) => ({ ...t, [key]: color }));
    };

    return (
      <Item>
        <ListItemText primary={label} />

        <MuiColorInput
          {...textProps}
          value={color}
          fallbackValue={dt[key]}
          isAlphaHidden
          format="hex"
          onChange={onChange}
        />
      </Item>
    );
  };

  return (
    <BaseDialog
      open={open}
      title={t("Theme Setting")}
      okBtn={t("Save")}
      cancelBtn={t("Cancel")}
      contentSx={{ width: 400, maxHeight: "80%", overflow: "auto", pb: 0 }}
      onClose={() => setOpen(false)}
      onCancel={() => setOpen(false)}
      onOk={onSave}
    >
      <List sx={{ pt: 0 }}>
        {renderItem("Primary Color", "primary_color")}

        {renderItem("Secondary Color", "secondary_color")}

        {renderItem("Primary Text", "primary_text")}

        {renderItem("Secondary Text", "secondary_text")}

        {renderItem("Info Color", "info_color")}

        {renderItem("Error Color", "error_color")}

        {renderItem("Warning Color", "warning_color")}

        {renderItem("Success Color", "success_color")}

        <Item>
          <ListItemText primary="Font Family" />
          <TextField
            {...textProps}
            value={theme.font_family ?? ""}
            onChange={handleChange("font_family")}
            onKeyDown={(e) => e.key === "Enter" && onSave()}
          />
        </Item>

        <Item>
          <ListItemText primary="CSS Injection" />
          <TextField
            {...textProps}
            value={theme.css_injection ?? ""}
            onChange={handleChange("css_injection")}
            onKeyDown={(e) => e.key === "Enter" && onSave()}
          />
        </Item>
        <Item>
          {/* 单位为秒，内容为浮点数 */}
          <ListItemText primary="Page Transition Duration" />
          <TextField
            {...textProps}
            type="number"
            value={theme.page_transition_duration ?? ""}
            onChange={handleChange("page_transition_duration")}
            onKeyDown={(e) => e.key === "Enter" && onSave()}
          />
        </Item>
      </List>
    </BaseDialog>
  );
});

ThemeViewer.displayName = "ThemeViewer";

const Item = styled(ListItem)(() => ({
  padding: "5px 2px",
}));

const Round = styled("div")(() => ({
  width: "24px",
  height: "24px",
  borderRadius: "18px",
  display: "inline-block",
  marginRight: "8px",
}));
