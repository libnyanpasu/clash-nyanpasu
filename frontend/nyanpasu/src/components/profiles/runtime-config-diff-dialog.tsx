import { useAtomValue } from "jotai";
import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import useSWR from "swr";
import { monaco } from "@/services/monaco";
import { themeMode } from "@/store";
import { getRuntimeYaml, useClash } from "@nyanpasu/interface";
import { BaseDialog } from "@nyanpasu/ui";

export type RuntimeConfigDiffDialogProps = {
  open: boolean;
  onClose: () => void;
};

export default function RuntimeConfigDiffDialog({
  open,
  onClose,
}: RuntimeConfigDiffDialogProps) {
  const { t } = useTranslation();
  const { getProfiles, getProfileFile } = useClash();
  const currentProfileUid = getProfiles.data?.current;
  const mode = useAtomValue(themeMode);
  const {
    data: runtimeConfig,
    isLoading: isLoadingRuntimeConfig,
    error: errorRuntimeConfig,
  } = useSWR(open ? "/getRuntimeConfigYaml" : null, getRuntimeYaml);
  const {
    data: profileConfig,
    isLoading: isLoadingProfileConfig,
    error: errorProfileConfig,
  } = useSWR(
    open ? `/readProfileFile?uid=${currentProfileUid}` : null,
    async (key) => {
      const url = new URL(key, window.location.origin);
      return await getProfileFile(url.searchParams.get("uid")!);
    },
    {
      revalidateOnFocus: true,
      refreshInterval: 0,
    },
  );
  const monacoRef = useRef<typeof monaco | null>(null);
  const editorRef = useRef<monaco.editor.IStandaloneDiffEditor | null>(null);
  const domRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (open && runtimeConfig && profileConfig) {
      console.log("init monaco");
      const run = async () => {
        const { monaco } = await import("@/services/monaco");
        monacoRef.current = monaco;
        console.log(domRef.current);
        editorRef.current = monaco.editor.createDiffEditor(domRef.current!, {
          theme: mode === "light" ? "vs" : "vs-dark",
          minimap: { enabled: false },
          automaticLayout: true,
          readOnly: true,
        });
        editorRef.current.setModel({
          original: monaco.editor.createModel(profileConfig, "yaml"),
          modified: monaco.editor.createModel(runtimeConfig, "yaml"),
        });
      };
      run().catch(console.error);
    }
    return () => {
      monacoRef.current = null;
      editorRef.current?.dispose();
    };
  }, [mode, open, runtimeConfig, profileConfig]);
  console.log(currentProfileUid);
  if (!currentProfileUid) {
    return null;
  }

  return (
    <BaseDialog title={t("Runtime Config")} open={open} onClose={onClose}>
      <div className="xs:w-[95vw] h-full w-[80vw] px-4">
        <div ref={domRef} className="h-[75vh] w-full" />
      </div>
    </BaseDialog>
  );
}
