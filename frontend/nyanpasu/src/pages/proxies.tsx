import {
  Box,
  Button,
  ButtonGroup,
  TextField,
  alpha,
  useTheme,
} from "@mui/material";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNyanpasu, useClashCore, Clash } from "@nyanpasu/interface";
import { SidePage } from "@nyanpasu/ui";
import { DelayButton, GroupList, NodeList } from "@/components/proxies";
import { useAtom } from "jotai";
import { proxyGroupAtom } from "@/store";
import ContentDisplay from "@/components/base/content-display";
import SortSelector from "@/components/proxies/sort-selector";
import ProxyGroupName from "@/components/proxies/proxy-group-name";

export default function ProxyPage() {
  const { t } = useTranslation();

  const { getCurrentMode, setCurrentMode } = useNyanpasu();

  const { palette } = useTheme();

  const { data, updateGroupDelay } = useClashCore();

  const [proxyGroup] = useAtom(proxyGroupAtom);

  const [group, setGroup] =
    useState<Clash.Proxy<Clash.Proxy<string> | string>>();

  useEffect(() => {
    if (getCurrentMode.global) {
      setGroup(data?.global);
    } else if (getCurrentMode.direct) {
      setGroup(data?.direct);
    } else {
      if (proxyGroup.selector !== null) {
        setGroup(data?.groups[proxyGroup.selector]);
      }
    }
  }, [proxyGroup.selector, data?.groups, getCurrentMode]);

  const handleDelayClick = async () => {
    await updateGroupDelay(proxyGroup.selector as number);
  };

  const hasProxies = Boolean(data?.groups.length);

  return (
    <SidePage
      title={t("Proxy Groups")}
      header={
        <Box display="flex" alignItems="center" gap={1}>
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
      side={hasProxies && getCurrentMode.rule && <GroupList />}
      toolBar={
        hasProxies &&
        !getCurrentMode.direct && (
          <div className="w-full flex items-center justify-between">
            <div className="flex items-center gap-4">
              {group?.name && <ProxyGroupName name={group?.name} />}
            </div>

            <div>
              <SortSelector />
            </div>
          </div>
        )
      }
      noChildrenScroll
    >
      {!getCurrentMode.direct ? (
        hasProxies ? (
          <>
            <NodeList />

            <DelayButton onClick={handleDelayClick} />
          </>
        ) : (
          <ContentDisplay message="None Proxies" />
        )
      ) : (
        <ContentDisplay message="Direct Mode" />
      )}
    </SidePage>
  );
}
