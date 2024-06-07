import { BaseEmpty } from "@/components/base";
import ClearLogButton from "@/components/logs/clear-log-button";
import LogToggle from "@/components/logs/log-toggle";
import { atomLogData } from "@/store";
import { LogMessage } from "@nyanpasu/interface";
import { BasePage } from "@nyanpasu/ui";
import { useAtomValue } from "jotai";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { LogLevel } from "@/components/logs/log-level";
import { LogList } from "@/components/logs/log-list";
import { LogFilter } from "@/components/logs/log-filter";

export default function LogPage() {
  const { t } = useTranslation();

  const logData = useAtomValue(atomLogData);

  const [logState, setLogState] = useState("all");

  const [filterText, setFilterText] = useState("");

  const [filterLogs, setFilterLogs] = useState<LogMessage[]>([]);

  useEffect(() => {
    setFilterLogs(
      logData.filter((data) => {
        return (
          data.payload.includes(filterText) &&
          (logState === "all" ? true : data.type.includes(logState))
        );
      }),
    );
  }, [logData, logState, filterText]);

  return (
    <BasePage
      full
      title={t("Logs")}
      contentStyle={{ height: "100%" }}
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
    >
      {filterLogs.length ? (
        <LogList data={filterLogs} />
      ) : (
        <BaseEmpty text="No Logs" />
      )}

      <ClearLogButton />
    </BasePage>
  );
}
