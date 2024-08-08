import { useDebounceEffect } from "ahooks";
import { useAtomValue, useSetAtom } from "jotai";
import { RefObject, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { LogFilter } from "@/components/logs/log-filter";
import { LogLevel } from "@/components/logs/log-level";
import LogToggle from "@/components/logs/log-toggle";
import { atomLogList } from "@/components/logs/modules/store";
import { atomLogData } from "@/store";
import { BasePage } from "@nyanpasu/ui";

export default function LogPage() {
  const { t } = useTranslation();

  const logData = useAtomValue(atomLogData);

  const [logState, setLogState] = useState("all");

  const [filterText, setFilterText] = useState("");

  const setLogList = useSetAtom(atomLogList);

  const viewportRef = useRef<HTMLDivElement>(null);

  useDebounceEffect(
    () => {
      setLogList({
        data: logData.filter((data) => {
          return (
            data.payload.includes(filterText) &&
            (logState === "all" ? true : data.type.includes(logState))
          );
        }),
        scrollRef: viewportRef as RefObject<HTMLElement>,
      });
    },
    [logData, logState, filterText],
    { wait: 150 },
  );

  return (
    <BasePage
      full
      title={t("Logs")}
      header={
        <div className="flex gap-2">
          <LogToggle />

          <LogLevel value={logState} onChange={(value) => setLogState(value)} />

          <LogFilter
            value={filterText}
            onChange={(value) => setFilterText(value)}
          />
        </div>
      }
      viewportRef={viewportRef}
      children={() => import("@/components/logs/log-page")}
    />
  );
}
