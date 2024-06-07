import { useMessage } from "@/hooks/use-notification";
import LoadingButton from "@mui/lab/LoadingButton";
import { useClashCore } from "@nyanpasu/interface";
import { useLockFn } from "ahooks";
import { useState } from "react";
import { Refresh } from "@mui/icons-material";
import { useTranslation } from "react-i18next";

export const UpdateProviders = () => {
  const { t } = useTranslation();

  const [loading, setLoading] = useState(false);

  const { getRulesProviders, updateRulesProviders } = useClashCore();

  const handleProviderUpdate = useLockFn(async () => {
    if (!getRulesProviders.data) {
      useMessage(`No Providers.`, {
        type: "info",
        title: t("Info"),
      });

      return;
    }

    try {
      setLoading(true);

      const providers = Object.entries(getRulesProviders.data).map(
        ([name]) => name,
      );

      await Promise.all(
        providers.map((provider) => updateRulesProviders(provider)),
      );
    } catch (e) {
      useMessage(`Update all failed.\n${String(e)}`, {
        type: "error",
        title: t("Error"),
      });
    } finally {
      setLoading(false);
    }
  });

  return (
    <LoadingButton
      variant="contained"
      loading={loading}
      startIcon={<Refresh />}
      onClick={handleProviderUpdate}
    >
      {t("Update Rules Providers All")}
    </LoadingButton>
  );
};

export default UpdateProviders;
