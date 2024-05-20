import { ProviderButton } from "@/components/proxy/provider-button";
import {
  Box,
  Button,
  ButtonGroup,
  Menu,
  MenuItem,
  TextField,
  alpha,
  useTheme,
} from "@mui/material";
import { memo, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNyanpasu, useClashCore, Clash } from "@nyanpasu/interface";
import { SidePage } from "@nyanpasu/ui";
import { DelayButton, GroupList, NodeList } from "@/components/proxies";
import { Public } from "@mui/icons-material";
import { useAtom } from "jotai";
import { proxyGroupAtom, proxyGroupSortAtom } from "@/store";
import { AnimatePresence, motion } from "framer-motion";

const ContentDisplay = ({ message }: { message: string }) => (
  <div className="h-full w-full flex items-center justify-center">
    <div className="flex flex-col items-center gap-4">
      <Public className="!size-16" />
      <b>{message}</b>
    </div>
  </div>
);

const ProxyGroupName = memo(function ProxyGroupName({
  name,
}: {
  name: string;
}) {
  return (
    <AnimatePresence mode="sync">
      <motion.div
        key={`group-name-${name}`}
        className="absolute"
        initial={{ x: 100, opacity: 0 }}
        animate={{ x: 0, opacity: 1 }}
        exit={{ x: -100, opacity: 0 }}
      >
        {name}
      </motion.div>
    </AnimatePresence>
  );
});

const SortSelector = memo(function SortSelector() {
  const { t } = useTranslation();

  const [proxyGroupSort, setProxyGroupSort] = useAtom(proxyGroupSortAtom);

  type SortType = typeof proxyGroupSort;

  const [anchorEl, setAnchorEl] = useState<null | HTMLElement>(null);

  const handleClick = (sort: SortType) => {
    setAnchorEl(null);
    setProxyGroupSort(sort);
  };

  const tmaps: { [key: string]: string } = {
    default: "Sort by default",
    delay: "Sort by delay",
    name: "Sort by name",
  };

  return (
    <>
      <Button
        size="small"
        variant="outlined"
        sx={{ textTransform: "none" }}
        onClick={(e) => setAnchorEl(e.currentTarget)}
      >
        {t(tmaps[proxyGroupSort])}
      </Button>

      <Menu
        anchorEl={anchorEl}
        open={Boolean(anchorEl)}
        onClose={() => setAnchorEl(null)}
      >
        {Object.entries(tmaps).map(([key, value], index) => {
          return (
            <MenuItem key={index} onClick={() => handleClick(key as SortType)}>
              {t(value)}
            </MenuItem>
          );
        })}
      </Menu>
    </>
  );
});

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
