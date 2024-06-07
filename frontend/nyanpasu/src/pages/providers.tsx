import RulesProvider from "@/components/providers/rules-provider";
import UpdateProviders from "@/components/providers/update-providers";
import { Chip } from "@mui/material";
import Grid from "@mui/material/Unstable_Grid2";
import { useClashCore } from "@nyanpasu/interface";
import { BasePage } from "@nyanpasu/ui";
import { useTranslation } from "react-i18next";

export default function ProvidersPage() {
  const { t } = useTranslation();

  const { getRulesProviders } = useClashCore();

  return (
    <BasePage title={t("Providers")}>
      <div className="flex flex-col gap-4">
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
              <Grid sm={12} md={6} lg={4} xl={3} key={name}>
                <RulesProvider provider={provider} />
              </Grid>
            ))}
          </Grid>
        )}
      </div>
    </BasePage>
  );
}
