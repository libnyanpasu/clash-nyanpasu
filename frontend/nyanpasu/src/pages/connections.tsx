import { useThrottle } from "ahooks";
import { lazy, useState } from "react";
import { useTranslation } from "react-i18next";
import CloseConnectionsButton from "@/components/connections/close-connections-button";
import HeaderSearch from "@/components/connections/header-search";
import { BasePage } from "@nyanpasu/ui";

const ConnectionsTable = lazy(
  () => import("@/components/connections/connections-table"),
);

export const Connections = () => {
  const { t } = useTranslation();

  const [searchTerm, setSearchTerm] = useState<string>();

  const throttledSearchTerm = useThrottle(searchTerm, { wait: 150 });

  return (
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
      children={() => (
        <>
          <ConnectionsTable searchTerm={throttledSearchTerm} />
          <CloseConnectionsButton />
        </>
      )}
    ></BasePage>
  );
};

export default Connections;
