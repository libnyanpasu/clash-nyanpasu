import { useAsyncEffect } from "ahooks";
import { useAtomValue } from "jotai";
import { forwardRef, useImperativeHandle, useRef } from "react";
import type { monaco } from "@/services/monaco";
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

  return <div ref={monacoRef} className={className} />;
});
