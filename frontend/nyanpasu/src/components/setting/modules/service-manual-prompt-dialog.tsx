import { useAsyncEffect } from "ahooks";
import { useAtom, useSetAtom } from "jotai";
import { useState } from "react";
import useSWR from "swr";
import { OS } from "@/consts";
import { serviceManualPromptDialogAtom } from "@/store/service";
import { getShikiSingleton } from "@/utils/shiki";
import { getServiceInstallPrompt } from "@nyanpasu/interface";
import { BaseDialog, BaseDialogProps } from "@nyanpasu/ui";

export type ServerManualPromptDialogProps = Omit<BaseDialogProps, "title"> & {
  operation: "uninstall" | "install" | "start" | "stop" | null;
};

// TODO: maybe support more commands prompt?
export default function ServerManualPromptDialog({
  open,
  onClose,
  operation,
  ...props
}: ServerManualPromptDialogProps) {
  const { data: serviceInstallPrompt, error } = useSWR(
    operation === "install" ? "/service_install_prompt" : null,
    getServiceInstallPrompt,
  );
  const [codes, setCodes] = useState<string | null>(null);
  useAsyncEffect(async () => {
    if (operation === "install" && serviceInstallPrompt) {
      const shiki = await getShikiSingleton();
      const code = await shiki.codeToHtml(serviceInstallPrompt, {
        lang: "shell",
        themes: {
          dark: "nord",
          light: "min-light",
        },
      });
      setCodes(code);
    } else if (!!operation) {
      const shiki = await getShikiSingleton();
      const code = await shiki.codeToHtml(
        `${OS !== "windows" ? "sudo " : ""}./nyanpasu-service ${operation}`,
        {
          lang: "shell",
          themes: {
            dark: "nord",
            light: "min-light",
          },
        },
      );
      setCodes(code);
    }
  }, [serviceInstallPrompt, operation, setCodes]);

  return (
    <BaseDialog
      title="Service Manual Tips"
      open={open}
      onClose={onClose}
      {...props}
    >
      <div className="grid gap-3">
        <p>
          Unable to install service automatically. Please open a PowerShell(as
          administrator) in Windows or a terminal emulator in macOS, Linux and
          run the following commands:
        </p>
        {error && <p className="text-red-500">{error.message}</p>}
        {!!codes && (
          <div
            dangerouslySetInnerHTML={{
              __html: codes,
            }}
          />
        )}
      </div>
    </BaseDialog>
  );
}

export function ServerManualPromptDialogWrapper() {
  const [prompt, setPrompt] = useAtom(serviceManualPromptDialogAtom);
  return (
    <ServerManualPromptDialog
      open={!!prompt}
      onClose={() => setPrompt(null)}
      operation={prompt}
    />
  );
}

export function useServerManualPromptDialog() {
  const setPrompt = useSetAtom(serviceManualPromptDialogAtom);
  return {
    show: (prompt: "install" | "uninstall" | "stop" | "start") =>
      setPrompt(prompt),
    close: () => setPrompt(null),
  };
}
