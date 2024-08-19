import { useThrottle } from "ahooks";
import { lazy, useState } from "react";
import { useTranslation } from "react-i18next";
import { SearchTermCtx } from "@/components/connections/connection-search-term";
import HeaderSearch from "@/components/connections/header-search";
import { BasePage } from "@nyanpasu/ui";

const Component = lazy(
  () => import("@/components/connections/connection-page"),
);

export const Connections = () => {
  const { t } = useTranslation();

  const [searchTerm, setSearchTerm] = useState<string>();

  const throttledSearchTerm = useThrottle(searchTerm, { wait: 150 });

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
        <Component />
      </BasePage>
    </SearchTermCtx.Provider>
  );
};

export default Connections;
