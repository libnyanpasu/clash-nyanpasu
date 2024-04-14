import { BasePage } from "@/components/base";
import { ProviderButton } from "@/components/proxy/provider-button";
import { ProxyGroups } from "@/components/proxy/proxy-groups";
import { Box, Button, ButtonGroup } from "@mui/material";
import { useLockFn } from "ahooks";
import { useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useNyanpasu, useClash } from "@nyanpasu/interface";

export default function ProxyPage() {
  const { t } = useTranslation();

  const { nyanpasuConfig } = useNyanpasu();

  const { getConfigs, setConfigs, deleteConnections } = useClash();

  const modeList = useMemo(() => {
    const defaultModes = ["rule", "global", "direct"];

    return ["mihomo", "mihomo-alpha", "clash-rs"].includes(
      nyanpasuConfig?.clash_core,
    )
      ? defaultModes
      : [...defaultModes, "script"];
  }, [nyanpasuConfig?.clash_core]);

  const currentMode = getConfigs.data?.mode?.toLowerCase();

  const onChangeMode = useLockFn(async (mode) => {
    if (mode !== currentMode && nyanpasuConfig?.auto_close_connection) {
      await deleteConnections();
    }

    await setConfigs({ mode });
    await getConfigs.mutate();
  });

  useEffect(() => {
    if (currentMode && !modeList.includes(currentMode)) {
      onChangeMode("rule");
    }
  }, [currentMode, modeList, onChangeMode]);

  return (
    <BasePage
      full
      contentStyle={{ height: "100%" }}
      title={t("Proxy Groups")}
      header={
        <Box display="flex" alignItems="center" gap={1}>
          <ProviderButton />

          <ButtonGroup size="small">
            {modeList.map((mode) => (
              <Button
                key={mode}
                variant={mode === currentMode ? "contained" : "outlined"}
                onClick={() => onChangeMode(mode)}
                sx={{ textTransform: "capitalize" }}
              >
                {t(mode)}
              </Button>
            ))}
          </ButtonGroup>
        </Box>
      }
    >
      <ProxyGroups mode={currentMode!} />
    </BasePage>
  );
}
