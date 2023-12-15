import { BasePage } from "@/components/base";
import RulesProvider from "@/components/providers/rules-provider";
import { useNotification } from "@/hooks/use-notification";
import { getRulesProviders, updateRulesProviders } from "@/services/api";
import { Error as ErrorIcon } from "@mui/icons-material";
import { LoadingButton } from "@mui/lab";
import {
  Box,
  Button,
  CircularProgress,
  Stack,
  Typography,
} from "@mui/material";
import Grid from "@mui/material/Unstable_Grid2";
import { useLockFn } from "ahooks";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import useSWR from "swr";

export default function ProvidersPage() {
  const { t } = useTranslation();
  const {
    data: rulesProviders,
    error: rulesProvidersError,
    isLoading: rulesProvidersLoading,
    mutate: mutateRulesProviders,
  } = useSWR("getClashProvidersRules", getRulesProviders);

  const [updating, setUpdating] = useState(false);
  const onUpdateAll = useLockFn(async () => {
    setUpdating(true);
    try {
      const queue = rulesProviders!.map((provider) =>
        updateRulesProviders(provider.name),
      );
      await Promise.all(queue);
      await mutateRulesProviders();
      useNotification(t("Success"), t("Update Rules Providers Success"));
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
    } catch (err: any) {
      useNotification(t("Error"), err.message || err.toString());
    } finally {
      setUpdating(false);
    }
  });

  const onRulesProviderUpdated = () => {
    mutateRulesProviders();
  };

  return (
    <BasePage title={t("Providers")}>
      <Box display="flex" justifyContent="space-between" alignItems="center">
        <Typography
          variant="h5"
          style={{
            margin: 0,
          }}
        >
          {t("Rules Providers")}
        </Typography>
        <LoadingButton
          variant="contained"
          loading={updating}
          disabled={updating}
          onClick={onUpdateAll}
        >
          {t("Update Rules Providers All")}
        </LoadingButton>
      </Box>
      <Stack
        spacing={2}
        sx={{
          marginTop: 2,
        }}
      >
        {
          /* 这里需要抽象个状态机组件出来 */
          rulesProvidersLoading || rulesProvidersError ? (
            <Box
              display="flex"
              justifyContent="center"
              alignItems="center"
              sx={{
                height: 200,
              }}
            >
              {rulesProvidersLoading ? (
                <CircularProgress />
              ) : (
                <Stack spacing={3} alignItems="center">
                  <h3
                    style={{
                      textAlign: "center",
                    }}
                  >
                    {t("Error")}
                  </h3>
                  <ErrorIcon
                    color="error"
                    sx={{
                      fontSize: "4em",
                    }}
                  />
                  <Button
                    variant="outlined"
                    onClick={() => mutateRulesProviders()}
                  >
                    {t("Retry")}
                  </Button>
                </Stack>
              )}
            </Box>
          ) : (
            <Grid
              container
              spacing={2}
              style={{
                marginTop: "0.8em",
              }}
            >
              {rulesProviders!.map((provider) => (
                <Grid xs={12} md={6} xl={4} key={provider.name}>
                  <RulesProvider
                    provider={provider}
                    onRulesProviderUpdated={onRulesProviderUpdated}
                  />
                </Grid>
              ))}
            </Grid>
          )
        }
      </Stack>
    </BasePage>
  );
}
