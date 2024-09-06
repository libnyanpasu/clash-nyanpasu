import { useUpdateEffect } from "ahooks";
import { useAtomValue } from "jotai";
import { nanoid } from "nanoid";
import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import { OS } from "@/consts";
import { monaco } from "@/services/monaco";
import { themeMode } from "@/store";

export interface ProfileMonacoViewProps {
  open: boolean;
  value?: string;
  language?: string;
  className?: string;
  readonly?: boolean;
  schemaType?: "clash" | "merge";
}

export interface ProfileMonacoViewRef {
  getValue: () => string | undefined;
}

export const ProfileMonacoView = forwardRef(function ProfileMonacoView(
  {
    open,
    value,
    language,
    readonly = false,
    schemaType,
    className,
  }: ProfileMonacoViewProps,
  ref,
) {
  const mode = useAtomValue(themeMode);

  const monacoRef = useRef<HTMLDivElement>(null);

  const monacoEditorRef = useRef<typeof monaco | null>(null);

  const instanceRef = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);

  useEffect(() => {
    const run = async () => {
      const { monaco } = await import("@/services/monaco");
      monacoEditorRef.current = monaco;

      if (!monacoRef.current) {
        return;
      }

      instanceRef.current = monaco.editor.create(monacoRef.current, {
        readOnly: readonly,
        renderValidationDecorations: "on",
        theme: mode === "light" ? "vs" : "vs-dark",
        tabSize: language === "yaml" ? 2 : 4,
        minimap: { enabled: false },
        automaticLayout: true,
        fontLigatures: true,
        smoothScrolling: true,
        fontFamily: `'Cascadia Code NF', 'Cascadia Code', Fira Code, JetBrains Mono, Roboto Mono, "Source Code Pro", Consolas, Menlo, Monaco, monospace, "Courier New", "Apple Color Emoji"${
          OS === "windows" ? ", twemoji mozilla" : ""
        }`,
        quickSuggestions: {
          strings: true,
          comments: true,
          other: true,
        },
      });
      const uri = monaco.Uri.parse(
        `${nanoid()}.${!!schemaType ? `${schemaType}.` : ""}.${language}`,
      );
      const model = monaco.editor.createModel(value || "", language, uri);
      instanceRef.current.setModel(model);
    };
    if (open) {
      run().catch(console.error);
    }
    return () => {
      instanceRef.current?.dispose();
    };
  }, [language, mode, open, readonly, schemaType, value]);

  useImperativeHandle(ref, () => ({
    getValue: () => instanceRef.current?.getValue(),
  }));

  useUpdateEffect(() => {
    const model = instanceRef.current?.getModel();

    if (!model || !language) {
      return;
    }

    monacoEditorRef.current?.editor.setModelLanguage(model, language);
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
