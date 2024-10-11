import { useLockFn, useReactive } from "ahooks";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { message } from "@/utils/notification";
import { Done } from "@mui/icons-material";
import {
  Box,
  Button,
  InputAdornment,
  List,
  ListItem,
  TextField,
  Typography,
} from "@mui/material";
import Grid from "@mui/material/Grid2";
import { useNyanpasu } from "@nyanpasu/interface";
import {
  BaseCard,
  Expand,
  ExpandMore,
  NumberItem,
  SwitchItem,
} from "@nyanpasu/ui";
import { PaperSwitchButton } from "./modules/system-proxy";

export const SettingSystemProxy = () => {
  const { t } = useTranslation();

  const { nyanpasuConfig, setNyanpasuConfig, getSystemProxy } = useNyanpasu();

  const loading = useReactive({
    enable_tun_mode: false,
    enable_system_proxy: false,
  });

  const handleClick = useLockFn(
    async (key: "enable_system_proxy" | "enable_tun_mode") => {
      try {
        loading[key] = true;

        await setNyanpasuConfig({
          [key]: !nyanpasuConfig?.[key],
        });
      } catch (e) {
        message(`Activation failed!`, {
          title: t("Error"),
          kind: "error",
        });
      } finally {
        loading[key] = false;
      }
    },
  );

  const [expand, setExpand] = useState(false);

  const [proxyBypass, setProxyBypass] = useState(
    nyanpasuConfig?.system_proxy_bypass || "",
  );

  return (
    <BaseCard
      label={t("System Setting")}
      labelChildren={
        <ExpandMore expand={expand} onClick={() => setExpand(!expand)} />
      }
    >
      <Grid container spacing={2}>
        <Grid
          size={{
            xs: 6,
          }}
        >
          <PaperSwitchButton
            label={t("Tun Mode")}
            checked={nyanpasuConfig?.enable_tun_mode || false}
            loading={loading.enable_tun_mode}
            onClick={() => handleClick("enable_tun_mode")}
          />
        </Grid>

        <Grid size={{ xs: 6 }}>
          <PaperSwitchButton
            label={t("System Proxy")}
            checked={nyanpasuConfig?.enable_system_proxy || false}
            loading={loading.enable_system_proxy}
            onClick={() => handleClick("enable_system_proxy")}
          />
        </Grid>
      </Grid>

      <Expand open={expand}>
        <List disablePadding sx={{ pt: 1 }}>
          <SwitchItem
            label={t("Proxy Guard")}
            checked={nyanpasuConfig?.enable_proxy_guard || false}
            onChange={() =>
              setNyanpasuConfig({
                enable_proxy_guard: !nyanpasuConfig?.enable_proxy_guard,
              })
            }
          />

          <NumberItem
            label={t("Guard Duration")}
            vaule={nyanpasuConfig?.proxy_guard_duration || 0}
            checkEvent={(input) => input <= 0}
            checkLabel="Dueation must be greater than 0."
            onApply={(value) => {
              setNyanpasuConfig({ proxy_guard_duration: value });
            }}
            textFieldProps={{
              inputProps: {
                "aria-autocomplete": "none",
              },
              InputProps: {
                endAdornment: <InputAdornment position="end">s</InputAdornment>,
              },
            }}
          />

          <ListItem sx={{ pl: 0, pr: 0 }}>
            <TextField
              value={proxyBypass}
              label={t("Proxy Bypass")}
              variant="outlined"
              sx={{ width: "100%" }}
              multiline
              onChange={(e) => setProxyBypass(e.target.value)}
            />
          </ListItem>

          <Expand open={proxyBypass != nyanpasuConfig?.system_proxy_bypass}>
            <Box sx={{ pb: 1 }} display="flex" justifyContent="end">
              <Button
                variant="contained"
                startIcon={<Done />}
                onClick={() => {
                  setNyanpasuConfig({ system_proxy_bypass: proxyBypass });
                }}
              >
                {t("Apply")}
              </Button>
            </Box>
          </Expand>

          <ListItem sx={{ pl: 0, pr: 0 }}>
            <Box>
              <Typography variant="body1" sx={{ fontSize: "18px", mb: 1 }}>
                {t("Current System Proxy")}
              </Typography>

              {Object.entries(getSystemProxy?.data ?? []).map(
                ([key, value], index) => {
                  return (
                    <Box key={index} display="flex" sx={{ pt: 1 }}>
                      <Typography
                        sx={{ width: 80, textTransform: "capitalize" }}
                      >
                        {key}:
                      </Typography>

                      <Typography>{String(value)}</Typography>
                    </Box>
                  );
                },
              )}
            </Box>
          </ListItem>
        </List>
      </Expand>
    </BaseCard>
  );
};

export default SettingSystemProxy;
