import { useUpdateEffect } from "ahooks";
import { useAtomValue } from "jotai";
import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import { monaco } from "@/services/monaco";
import { themeMode } from "@/store";

export interface ProfileMonacoViewProps {
  open: boolean;
  value?: string;
  language?: string;
  className?: string;
}

export interface ProfileMonacoViewRef {
  getValue: () => string | undefined;
}

export const ProfileMonacoView = forwardRef(function ProfileMonacoView(
  { open, value, language, className }: ProfileMonacoViewProps,
  ref,
) {
  const mode = useAtomValue(themeMode);

  const monacoRef = useRef<HTMLDivElement>(null);

  const monacoeditorRef = useRef<typeof monaco | null>(null);

  const instanceRef = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);

  useEffect(() => {
    const run = async () => {
      const { monaco } = await import("@/services/monaco");
      monacoeditorRef.current = monaco;

      if (!monacoRef.current) {
        return;
      }

      instanceRef.current = monaco.editor.create(monacoRef.current, {
        value,
        language,
        theme: mode === "light" ? "vs" : "vs-dark",
        minimap: { enabled: false },
        automaticLayout: true,
      });
    };
    if (open) {
      run().catch(console.error);
    }
    return () => {
      instanceRef.current?.dispose();
    };
  }, [open]);

  useImperativeHandle(ref, () => ({
    getValue: () => instanceRef.current?.getValue(),
  }));

  useUpdateEffect(() => {
    const model = instanceRef.current?.getModel();

    if (!model || !language) {
      return;
    }

    monacoeditorRef.current?.editor.setModelLanguage(model, language);
  }, [language]);

  useUpdateEffect(() => {
    const model = instanceRef.current?.getModel();

    if (!model || !value) {
      return;
    }

    model.setValue(value);
  }, [value]);

  return <div ref={monacoRef} className={className} />;
});
