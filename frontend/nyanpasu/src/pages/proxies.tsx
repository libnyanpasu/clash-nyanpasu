import { useAtom } from "jotai";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import ContentDisplay from "@/components/base/content-display";
import {
  DelayButton,
  GroupList,
  NodeList,
  NodeListRef,
} from "@/components/proxies";
import ProxyGroupName from "@/components/proxies/proxy-group-name";
import ScrollCurrentNode from "@/components/proxies/scroll-current-node";
import SortSelector from "@/components/proxies/sort-selector";
import { proxyGroupAtom } from "@/store";
import {
  alpha,
  Box,
  Button,
  ButtonGroup,
  TextField,
  useTheme,
} from "@mui/material";
import { Clash, useClashCore, useNyanpasu } from "@nyanpasu/interface";
import { SidePage } from "@nyanpasu/ui";

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
  }, [
    proxyGroup.selector,
    data?.groups,
    getCurrentMode,
    data?.global,
    data?.direct,
  ]);

  const handleDelayClick = async () => {
    await updateGroupDelay(proxyGroup.selector as number);
  };

  const hasProxies = Boolean(data?.groups.length);

  const nodeListRef = useRef<NodeListRef>(null);

  const Header = () => {
    const handleSwitch = (key: string) => {
      setCurrentMode(key);
    };

    return (
      <Box display="flex" alignItems="center" gap={1}>
        <ButtonGroup size="small">
          {Object.entries(getCurrentMode).map(([key, value], index) => (
            <Button
              key={index}
              variant={value ? "contained" : "outlined"}
              onClick={() => handleSwitch(key)}
              sx={{ textTransform: "capitalize" }}
            >
              {t(key)}
            </Button>
          ))}
        </ButtonGroup>
      </Box>
    );
  };

  const SideBar = () => {
    return (
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
    );
  };

  return (
    <SidePage
      title={t("Proxy Groups")}
      header={<Header />}
      sideBar={<SideBar />}
      side={hasProxies && getCurrentMode.rule && <GroupList />}
      toolBar={
        hasProxies &&
        !getCurrentMode.direct && (
          <div className="flex w-full items-center justify-between">
            <div className="flex items-center gap-4">
              {group?.name && <ProxyGroupName name={group?.name} />}
            </div>

            <div className="flex gap-2">
              <ScrollCurrentNode
                onClick={() => {
                  nodeListRef.current?.scrollToCurrent();
                }}
              />

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
            <NodeList ref={nodeListRef} />

            <DelayButton onClick={handleDelayClick} />
          </>
        ) : (
          <ContentDisplay message="No Proxy" />
        )
      ) : (
        <ContentDisplay message="Direct Mode" />
      )}
    </SidePage>
  );
}
