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
import { useReactive } from "ahooks";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useNyanpasu, useClashCore } from "@nyanpasu/interface";
import { SidePage } from "@nyanpasu/ui";
import { GroupList, NodeList } from "@/components/proxies";
import { Bolt, Public } from "@mui/icons-material";
import { useAtom } from "jotai";
import { proxyGroupAtom } from "@/store";
import LoadingButton from "@mui/lab/LoadingButton";

export default function ProxyPage() {
  const { t } = useTranslation();

  const { getCurrentMode, setCurrentMode } = useNyanpasu();

  const { palette } = useTheme();

  const { data, updateGroupDelay } = useClashCore();

  const [proxyGroup] = useAtom(proxyGroupAtom);

  const loading = useReactive({
    delay: false,
  });

  const group = useMemo(() => {
    if (getCurrentMode.global) {
      return data?.global;
    } else if (getCurrentMode.direct) {
      return data?.direct;
    } else {
      if (proxyGroup.selector !== null) {
        return data?.groups[proxyGroup.selector];
      } else {
        return undefined;
      }
    }
  }, [proxyGroup.selector, data?.groups, getCurrentMode]);

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
            {Object.entries(getCurrentMode).map(([key, value], index) => (
              <Button
                key={index}
                variant={value ? "contained" : "outlined"}
                onClick={() => setCurrentMode(key)}
                sx={{ textTransform: "capitalize" }}
              >
                {t(key)}
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
      side={getCurrentMode.rule && <GroupList />}
      toolBar={
        !getCurrentMode.direct && (
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
        )
      }
    >
      {!getCurrentMode.direct ? (
        <>
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
        </>
      ) : (
        <div className="h-full w-full flex items-center justify-center">
          <div className="flex flex-col items-center gap-4">
            <Public className="!size-16" />
            <b>Direct Mode</b>
          </div>
        </div>
      )}
    </SidePage>
  );
}
