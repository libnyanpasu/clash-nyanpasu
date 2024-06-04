import CloseConnectionsButton from "@/components/connections/close-connections-button";
import ConnectionsTable from "@/components/connections/connections-table";
import { BasePage } from "@nyanpasu/ui";
import { useTranslation } from "react-i18next";

export const Connections = () => {
  const { t } = useTranslation();

  return (
    <BasePage
      title={t("Connections")}
      full
      header={
        <div className=" max-h-96">
          <div id="filter-panel" />
        </div>
      }
    >
      <ConnectionsTable />

      <CloseConnectionsButton />
    </BasePage>
  );
};

export default Connections;
