import { useAsyncEffect } from "ahooks";
import { useAtom } from "jotai";
import { useState } from "react";
import useSWR from "swr";
import { serviceManualPromptDialogAtom } from "@/store/service";
import { getShikiSingleton } from "@/utils/shiki";
import { getServiceInstallPrompt } from "@nyanpasu/interface";
import { BaseDialog, BaseDialogProps } from "@nyanpasu/ui";

export type ServerManualPromptDialogProps = Omit<BaseDialogProps, "title">;

// TODO: maybe support more commands prompt?
export default function ServerManualPromptDialog({
  open,
  onClose,
  ...props
}: ServerManualPromptDialogProps) {
  const { data: serviceInstallPrompt } = useSWR(
    "/service_install_prompt",
    getServiceInstallPrompt,
  );
  const [codes, setCodes] = useState<string | null>(null);
  useAsyncEffect(async () => {
    if (serviceInstallPrompt) {
      const shiki = await getShikiSingleton();
      const code = await shiki.codeToHtml(serviceInstallPrompt, {
        lang: "shell",
        themes: {
          dark: "nord",
          light: "min-light",
        },
      });
      setCodes(code);
    }
  }, [serviceInstallPrompt]);

  return (
    <BaseDialog title="Server Manual" open={open} onClose={onClose} {...props}>
      <div className="grid gap-3">
        <p>
          Unable to install service automatically. Please open a PowerShell(as
          administrator) in Windows or a terminal emulator in macOS, Linux and
          run the following commands:
        </p>
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
  const [open, setOpen] = useAtom(serviceManualPromptDialogAtom);
  return (
    <ServerManualPromptDialog open={open} onClose={() => setOpen(false)} />
  );
}

export function useServerManualPromptDialog() {
  const [, setOpen] = useAtom(serviceManualPromptDialogAtom);
  return {
    show: () => setOpen(true),
    close: () => setOpen(false),
  };
}
