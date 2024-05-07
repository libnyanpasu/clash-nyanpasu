import { ProviderButton } from "@/components/proxy/provider-button";
import {
  Box,
  Button,
  ButtonGroup,
  TextField,
  Typography,
  alpha,
  useTheme,
} from "@mui/material";
import { useLockFn, useReactive } from "ahooks";
import { useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useNyanpasu, useClash, useClashCore } from "@nyanpasu/interface";
import { SidePage } from "@nyanpasu/ui";
import { GroupList, NodeList } from "@/components/proxies";
import { Bolt } from "@mui/icons-material";
import { useAtom } from "jotai";
import { proxyGroupAtom } from "@/store";
import LoadingButton from "@mui/lab/LoadingButton";

export default function ProxyPage() {
  const { t } = useTranslation();

  const { nyanpasuConfig } = useNyanpasu();

  const { getConfigs, setConfigs, deleteConnections } = useClash();

  const modeList = useMemo(() => {
    const defaultModes = ["rule", "global", "direct"];

    return ["mihomo", "mihomo-alpha", "clash-rs"].includes(
      nyanpasuConfig?.clash_core as string,
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

  const { palette } = useTheme();

  const { data, updateGroupDelay } = useClashCore();

  const [proxyGroup] = useAtom(proxyGroupAtom);

  const loading = useReactive({
    delay: false,
  });

  const group = useMemo(() => {
    if (proxyGroup.selector !== null) {
      return data?.groups[proxyGroup.selector];
    } else {
      return undefined;
    }
  }, [proxyGroup.selector]);

  const handleDelayClick = async () => {
    try {
      loading.delay = true;

      await updateGroupDelay(proxyGroup.selector as number);
    } finally {
      loading.delay = false;
    }
  };

  return (
    <SidePage
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
      sideBar={
        <TextField
          hiddenLabel
          fullWidth
          autoComplete="off"
          spellCheck="false"
          placeholder={t("Filter conditions")}
          sx={{ input: { py: 1, px: 2 } }}
          InputProps={{
            sx: {
              borderRadius: 7,
              backgroundColor: alpha(palette.primary.main, 0.1),
            },
          }}
        />
      }
      side={<GroupList />}
      toolBar={
        <Box
          width="100%"
          display="flex"
          alignItems="center"
          justifyContent="space-between"
        >
          <Box>
            <Typography>{group?.name}</Typography>
          </Box>
        </Box>
      }
    >
      <NodeList />

      <LoadingButton
        size="large"
        sx={{
          position: "fixed",
          bottom: 32,
          right: 32,
          zIndex: 10,
          height: 64,
          width: 64,
          borderRadius: 4,
          boxShadow: 8,
          backgroundColor: alpha(palette.primary.main, 0.3),
          backdropFilter: "blur(8px)",

          "&:hover": {
            backgroundColor: alpha(palette.primary.main, 0.1),
          },

          "&.MuiLoadingButton-loading": {
            backgroundColor: alpha(palette.primary.main, 0.15),
          },
        }}
        loading={loading.delay}
        onClick={handleDelayClick}
      >
        <Bolt />
      </LoadingButton>
    </SidePage>
  );
}
