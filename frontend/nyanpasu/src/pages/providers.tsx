import ProxiesProvider from "@/components/providers/proxies-provider";
import RulesProvider from "@/components/providers/rules-provider";
import UpdateProviders from "@/components/providers/update-providers";
import UpdateProxiesProviders from "@/components/providers/update-proxies-providers";
import { Chip } from "@mui/material";
import Grid from "@mui/material/Unstable_Grid2";
import { useClashCore } from "@nyanpasu/interface";
import { BasePage } from "@nyanpasu/ui";
import { useTranslation } from "react-i18next";

export default function ProvidersPage() {
  const { t } = useTranslation();

  const { getRulesProviders, getProxiesProviders } = useClashCore();

  return (
    <BasePage title={t("Providers")}>
      <div className="flex flex-col gap-4">
        <div className="flex justify-between items-center">
          <Chip
            className="font-bold !text-lg truncate !p-2 !h-10 !rounded-full"
            label={`${t("Proxies Providers")} (${Object.entries(getProxiesProviders.data ?? {}).length})`}
          />

          <UpdateProxiesProviders />
        </div>

        {getProxiesProviders.data && (
          <Grid container spacing={2}>
            {Object.entries(getProxiesProviders.data).map(
              ([name, provider]) => (
                <Grid
                  key={name}
                  className="w-full"
                  sm={12}
                  md={6}
                  lg={4}
                  xl={3}
                >
                  <ProxiesProvider provider={provider} />
                </Grid>
              ),
            )}
          </Grid>
        )}

        <div className="flex justify-between items-center">
          <Chip
            className="font-bold !text-lg truncate !p-2 !h-10 !rounded-full"
            label={`${t("Rules Providers")} (${Object.entries(getRulesProviders.data ?? {}).length})`}
          />

          <UpdateProviders />
        </div>

        {getRulesProviders.data && (
          <Grid container spacing={2}>
            {Object.entries(getRulesProviders.data).map(([name, provider]) => (
              <Grid key={name} className="w-full" sm={12} md={6} lg={4} xl={3}>
                <RulesProvider provider={provider} />
              </Grid>
            ))}
          </Grid>
        )}
      </div>
    </BasePage>
  );
}
