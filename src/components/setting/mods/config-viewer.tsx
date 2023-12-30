import { BaseDialog, DialogRef } from "@/components/base";
import { getRuntimeYaml } from "@/services/cmds";
import { atomThemeMode } from "@/services/states";
import { Chip } from "@mui/material";
import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import { useRecoilValue } from "recoil";

import { monaco } from "@/services/monaco";

export const ConfigViewer = forwardRef<DialogRef>((props, ref) => {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);

  const editorRef = useRef<any>();
  const instanceRef = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  const themeMode = useRecoilValue(atomThemeMode);

  useEffect(() => {
    return () => {
      if (instanceRef.current) {
        instanceRef.current.dispose();
        instanceRef.current = null;
      }
    };
  }, []);

  useImperativeHandle(ref, () => ({
    open: () => {
      setOpen(true);

      getRuntimeYaml().then((data) => {
        const dom = editorRef.current;

        if (!dom) return;
        if (instanceRef.current) instanceRef.current.dispose();

        instanceRef.current = monaco.editor.create(editorRef.current, {
          value: data ?? "# Error\n",
          language: "yaml",
          theme: themeMode === "light" ? "vs" : "vs-dark",
          minimap: { enabled: false },
          readOnly: true,
        });
      });
    },
    close: () => setOpen(false),
  }));

  return (
    <BaseDialog
      open={open}
      title={
        <>
          {t("Runtime Config")} <Chip label={t("ReadOnly")} size="small" />
        </>
      }
      contentSx={{ width: 520, pb: 1, userSelect: "text" }}
      cancelBtn={t("Back")}
      disableOk
      onClose={() => setOpen(false)}
      onCancel={() => setOpen(false)}
    >
      <div style={{ width: "100%", height: "420px" }} ref={editorRef} />
    </BaseDialog>
  );
});

ConfigViewer.displayName = "ConfigViewer";
