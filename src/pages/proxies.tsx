import { BasePage } from "@/components/base";
import { ProviderButton } from "@/components/proxy/provider-button";
import { ProxyGroups } from "@/components/proxy/proxy-groups";
import { useVerge } from "@/hooks/use-verge";
import {
  closeAllConnections,
  getClashConfig,
  updateConfigs,
} from "@/services/api";
import { patchClashConfig } from "@/services/cmds";
import { Box, Button, ButtonGroup, Paper } from "@mui/material";
import { useLockFn } from "ahooks";
import { useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";
import useSWR from "swr";

export default function ProxyPage() {
  const { t } = useTranslation();

  const { data: clashConfig, mutate: mutateClash } = useSWR(
    "getClashConfig",
    getClashConfig,
  );

  const { verge } = useVerge();

  const modeList = useMemo(() => {
    if (
      verge?.clash_core === "mihomo" ||
      verge?.clash_core === "mihomo-alpha" ||
      verge?.clash_core === "clash-rs"
    ) {
      return ["rule", "global", "direct"];
    }
    return ["rule", "global", "direct", "script"];
  }, [verge?.clash_core]);

  const curMode = clashConfig?.mode?.toLowerCase();

  const onChangeMode = useLockFn(async (mode: string) => {
    // 断开连接
    if (mode !== curMode && verge?.auto_close_connection) {
      closeAllConnections();
    }
    await updateConfigs({ mode });
    await patchClashConfig({ mode });
    mutateClash();
  });

  useEffect(() => {
    if (curMode && !modeList.includes(curMode)) {
      onChangeMode("rule");
    }
  }, [curMode]);

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
                variant={mode === curMode ? "contained" : "outlined"}
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
      <ProxyGroups mode={curMode!} />
    </BasePage>
  );
}
