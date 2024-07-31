import { useAsyncEffect, useLockFn, useUpdateEffect } from "ahooks";
import { useAtomValue } from "jotai";
import { forwardRef, useImperativeHandle, useRef } from "react";
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

  const instanceRef = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);

  useAsyncEffect(async () => {
    if (open) {
      const { monaco } = await import("@/services/monaco");

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
    } else {
      instanceRef.current?.dispose();
    }
  }, [open]);

  useImperativeHandle(ref, () => ({
    getValue: () => instanceRef.current?.getValue(),
  }));

  const changeLanguage = useLockFn(async () => {
    const { monaco } = await import("@/services/monaco");

    const text = instanceRef.current?.getModel();

    if (!text || !language) {
      return;
    }

    monaco.editor.setModelLanguage(text, language);
  });

  useUpdateEffect(() => {
    changeLanguage();
  }, [language]);

  return <div ref={monacoRef} className={className} />;
});
