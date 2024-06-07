import { useMessage } from "@/hooks/use-notification";
import { Refresh } from "@mui/icons-material";
import LoadingButton from "@mui/lab/LoadingButton/LoadingButton";
import { Chip, Paper } from "@mui/material";
import { ProviderRules, useClashCore } from "@nyanpasu/interface";
import { useLockFn } from "ahooks";
import dayjs from "dayjs";
import { useState } from "react";
import { useTranslation } from "react-i18next";

export interface RulesProviderProps {
  provider: ProviderRules;
}

export default function RulesProvider({ provider }: RulesProviderProps) {
  const { t } = useTranslation();

  const [loading, setLoading] = useState(false);

  const { updateRulesProviders } = useClashCore();

  const handleClick = useLockFn(async () => {
    try {
      setLoading(true);

      await updateRulesProviders(provider.name);
    } catch (e) {
      useMessage(`Update ${provider.name} failed.\n${String(e)}`, {
        type: "error",
        title: t("Error"),
      });
    } finally {
      setLoading(false);
    }
  });

  return (
    <Paper
      className="p-5 flex flex-col gap-2"
      sx={{
        borderRadius: 6,
      }}
    >
      <div className="flex items-start justify-between gap-2">
        <div className="ml-1">
          <p className="text-lg font-bold truncate">{provider.name}</p>

          <p className="truncate text-sm">
            {provider.vehicleType}/{provider.behavior}
          </p>
        </div>

        <div className="text-sm text-right">
          {t("Last Update", {
            fromNow: dayjs(provider.updatedAt).fromNow(),
          })}
        </div>
      </div>

      <div className="flex items-center justify-between">
        <Chip
          className="font-bold truncate"
          label={t("Rule Set rules", {
            rule: provider.ruleCount,
          })}
        />

        <LoadingButton
          loading={loading}
          size="small"
          variant="contained"
          className="!size-8 !min-w-0"
          onClick={handleClick}
        >
          <Refresh />
        </LoadingButton>
      </div>
    </Paper>
  );
}
