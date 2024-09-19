import { useThrottle } from "ahooks";
import { lazy, useDeferredValue, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { SearchTermCtx } from "@/components/connections/connection-search-term";
import HeaderSearch from "@/components/connections/header-search";
import { BasePage } from "@nyanpasu/ui";
import { createFileRoute, useBlocker } from "@tanstack/react-router";

const Component = lazy(
  () => import("@/components/connections/connection-page"),
);

export const Route = createFileRoute("/connections")({
  component: Connections,
});

function Connections() {
  const { t } = useTranslation();

  const [searchTerm, setSearchTerm] = useState<string>();
  const throttledSearchTerm = useThrottle(searchTerm, { wait: 150 });

  const [mountTable, setMountTable] = useState(true);
  const deferredMountTable = useDeferredValue(mountTable);
  const { proceed } = useBlocker({
    blockerFn: () => setMountTable(false),
    condition: !mountTable,
  });

  useEffect(() => {
    if (!deferredMountTable) {
      proceed();
    }
  }, [proceed, deferredMountTable]);

  return (
    <SearchTermCtx.Provider value={throttledSearchTerm}>
      <BasePage
        title={t("Connections")}
        full
        header={
          <div className="max-h-96">
            <HeaderSearch
              value={searchTerm}
              onChange={(e) => setSearchTerm(e.target.value)}
            />
          </div>
        }
      >
        {mountTable && <Component />}
      </BasePage>
    </SearchTermCtx.Provider>
  );
}
