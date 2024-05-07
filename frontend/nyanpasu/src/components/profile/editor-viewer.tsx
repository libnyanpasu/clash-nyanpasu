import { readProfileFile, saveProfileFile } from "@/services/cmds";
import {
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
} from "@mui/material";
import { useLockFn } from "ahooks";
import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { NotificationType, useNotification } from "@/hooks/use-notification";
import { monaco } from "@/services/monaco";
import { useAtomValue } from "jotai";
import { themeMode as atomThemeMode } from "@/store";

interface Props {
  uid: string;
  open: boolean;
  mode: "yaml" | "javascript";
  onClose: () => void;
  onChange?: () => void;
}

export const EditorViewer = (props: Props) => {
  const { uid, open, mode, onClose, onChange } = props;

  const { t } = useTranslation();
  const editorRef = useRef<any>();
  const instanceRef = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  const themeMode = useAtomValue(atomThemeMode);

  useEffect(() => {
    if (!open) return;

    readProfileFile(uid).then((data) => {
      const dom = editorRef.current;

      if (!dom) return;
      if (instanceRef.current) instanceRef.current.dispose();

      instanceRef.current = monaco.editor.create(editorRef.current, {
        value: data,
        language: mode,
        theme: themeMode === "light" ? "vs" : "vs-dark",
        minimap: { enabled: false },
      });
    });

    return () => {
      if (instanceRef.current) {
        instanceRef.current.dispose();
        instanceRef.current = null;
      }
    };
  }, [open]);

  const onSave = useLockFn(async () => {
    const value = instanceRef.current?.getValue();

    if (value == null) return;

    try {
      await saveProfileFile(uid, value);
      onChange?.();
      onClose();
    } catch (err: any) {
      useNotification({
        title: t("Error"),
        body: err.message || err.toString(),
        type: NotificationType.Error,
      });
    }
  });

  return (
    <Dialog open={open} onClose={onClose}>
      <DialogTitle>{t("Edit File")}</DialogTitle>

      <DialogContent sx={{ width: 520, pb: 1, userSelect: "text" }}>
        <div style={{ width: "100%", height: "420px" }} ref={editorRef} />
      </DialogContent>

      <DialogActions>
        <Button onClick={onClose} variant="outlined">
          {t("Cancel")}
        </Button>
        <Button onClick={onSave} variant="contained">
          {t("Save")}
        </Button>
      </DialogActions>
    </Dialog>
  );
};
