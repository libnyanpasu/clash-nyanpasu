import { lazy, RefObject, useRef } from "react";
import { useTranslation } from "react-i18next";
import LogHeader from "@/components/logs/los-header";
import { BasePage } from "@nyanpasu/ui";

export default function LogPage() {
  const { t } = useTranslation();

  const viewportRef = useRef<HTMLDivElement>(null);

  const Component = lazy(() => import("@/components/logs/log-page"));

  return (
    <BasePage
      full
      title={t("Logs")}
      header={<LogHeader />}
      viewportRef={viewportRef}
    >
      <Component scrollRef={viewportRef as RefObject<HTMLElement>} />
    </BasePage>
  );
}
