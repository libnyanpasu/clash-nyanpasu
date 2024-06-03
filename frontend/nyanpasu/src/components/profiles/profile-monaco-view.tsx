import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import { monaco } from "@/services/monaco";
import { useDebounceEffect, useUpdateEffect } from "ahooks";
import { themeMode } from "@/store";
import { useAtomValue } from "jotai";

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

  // wait for editor to be initialized
  useDebounceEffect(
    () => {
      const setupEditor = async () => {
        if (open && monacoRef.current) {
          instanceRef.current = monaco.editor.create(monacoRef.current, {
            value,
            language,
            theme: mode === "light" ? "vs" : "vs-dark",
            minimap: { enabled: false },
          });

          return () => {
            instanceRef.current?.dispose();
          };
        }
      };

      setupEditor();
    },
    [open],
    { wait: 100 },
  );

  useImperativeHandle(ref, () => ({
    getValue: () => instanceRef.current?.getValue(),
  }));

  useUpdateEffect(() => {
    if (!language) return;

    monaco.editor.setModelLanguage(instanceRef.current!.getModel()!, language);

    console.log(language, instanceRef.current?.getModel()?.getLanguageId());
  }, [language]);

  return open && <div ref={monacoRef} className={className} />;
});
