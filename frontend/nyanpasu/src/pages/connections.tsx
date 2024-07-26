import { useThrottle } from "ahooks";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import CloseConnectionsButton from "@/components/connections/close-connections-button";
import ConnectionsTable from "@/components/connections/connections-table";
import HeaderSearch from "@/components/connections/header-search";
import { BasePage } from "@nyanpasu/ui";

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
    >
      <ConnectionsTable searchTerm={throttledSearchTerm} />

      <CloseConnectionsButton />
    </BasePage>
  );
};

export default Connections;
