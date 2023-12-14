import { useNotification } from "@/hooks/use-notification";
import { updateRulesProviders, type ProviderRules } from "@/services/api";
import { Refresh } from "@mui/icons-material";
import {
  Box,
  Card,
  CardContent,
  CircularProgress,
  IconButton,
  Stack,
  Typography,
} from "@mui/material";
import { useLockFn } from "ahooks";
import dayjs from "dayjs";
import { useState } from "react";
import { useTranslation } from "react-i18next";

export interface RulesProviderProps {
  provider: ProviderRules;
  onRulesProviderUpdated: () => void;
}

export default function RulesProvider(props: RulesProviderProps) {
  const { provider, onRulesProviderUpdated } = props;
  const { t } = useTranslation();

  const [updating, setUpdating] = useState(false);
  const onUpdate = useLockFn(async () => {
    setUpdating(true);
    try {
      await updateRulesProviders(provider.name);
      onRulesProviderUpdated();
      useNotification(t("Success"), t("Update Rules Providers Success"));
    } catch (err: any) {
      useNotification(t("Error"), err.message || err.toString());
    } finally {
      setUpdating(false);
    }
  });

  return (
    <Card variant="outlined">
      <CardContent
        style={{
          position: "relative",
        }}
      >
        <IconButton
          onClick={onUpdate}
          disabled={updating}
          sx={{
            position: "absolute",
            top: "50%",
            right: "0.5em",
            transform: "translateY(-50%)",
          }}
          size="medium"
        >
          {updating ? (
            <CircularProgress size="1.11em" />
          ) : (
            <Refresh
              style={{
                fontSize: "1.11em",
              }}
            />
          )}
        </IconButton>

        <Stack gap={1}>
          <Box display="flex" gap={1} alignItems="end">
            <Typography
              variant="h6"
              component="div"
              sx={{
                padding: 0,
                margin: 0,
                lineHeight: "inherit",
              }}
            >
              <b>{provider.name}</b>
            </Typography>
            <Typography variant="caption" color="GrayText">
              {provider.vehicleType}/{provider.behavior}
            </Typography>
          </Box>

          <Stack>
            <Typography variant="body1">
              {t("Rule Set rules", {
                rule: provider.ruleCount,
              })}
            </Typography>
            <Typography variant="body2">
              {t("Last Update", {
                fromNow: dayjs(provider.updatedAt).fromNow(),
              })}
            </Typography>
          </Stack>
        </Stack>
      </CardContent>
    </Card>
  );
}
