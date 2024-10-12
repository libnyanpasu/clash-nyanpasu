import { useAtomValue } from "jotai";
import { nanoid } from "nanoid";
import { lazy, Suspense, useMemo } from "react";
import { useTranslation } from "react-i18next";
import useSWR from "swr";
import { themeMode } from "@/store";
import { getRuntimeYaml, useClash } from "@nyanpasu/interface";
import { BaseDialog, cn } from "@nyanpasu/ui";

const MonacoDiffEditor = lazy(() => import("./profile-monaco-diff-viewer"));

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
  const { data: runtimeConfig, isLoading: isLoadingRuntimeConfig } = useSWR(
    open ? "/getRuntimeConfigYaml" : null,
    getRuntimeYaml,
    {},
  );
  const { data: profileConfig, isLoading: isLoadingProfileConfig } = useSWR(
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

  const loaded = !isLoadingRuntimeConfig && !isLoadingProfileConfig;

  const originalModelPath = useMemo(() => `${nanoid()}.clash.yaml`, []);
  const modifiedModelPath = useMemo(() => `${nanoid()}.runtime.yaml`, []);

  if (!currentProfileUid) {
    return null;
  }

  return (
    <BaseDialog title={t("Runtime Config")} open={open} onClose={onClose}>
      <div className="xs:w-[95vw] h-full w-[80vw] px-4">
        <div
          className={cn(
            "items-center justify-between px-5 pb-2",
            loaded ? "flex" : "hidden",
          )}
        >
          <span className="text-base font-semibold">
            {t("Original Config")}
          </span>
          <span className="text-base font-semibold">{t("Runtime Config")}</span>
        </div>
        <div className="h-[75vh] w-full">
          <Suspense fallback={null}>
            {loaded && (
              <MonacoDiffEditor
                language="yaml"
                theme={mode === "light" ? "vs" : "vs-dark"}
                original={profileConfig}
                originalModelPath={originalModelPath}
                modified={runtimeConfig}
                modifiedModelPath={modifiedModelPath}
                options={{
                  minimap: { enabled: false },
                  automaticLayout: true,
                  readOnly: true,
                }}
              />
            )}
          </Suspense>
        </div>
      </div>
    </BaseDialog>
  );
}
