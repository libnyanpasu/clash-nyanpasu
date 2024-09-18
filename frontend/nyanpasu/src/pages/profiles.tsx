import { AnimatePresence, motion } from "framer-motion";
import { useAtom } from "jotai";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useWindowSize } from "react-use";
import { z } from "zod";
import {
  atomChainsSelected,
  atomGlobalChainCurrent,
} from "@/components/profiles/modules/store";
import NewProfileButton from "@/components/profiles/new-profile-button";
import {
  AddProfileContext,
  AddProfileContextValue,
} from "@/components/profiles/profile-dialog";
import ProfileItem from "@/components/profiles/profile-item";
import ProfileSide from "@/components/profiles/profile-side";
import { QuickImport } from "@/components/profiles/quick-import";
import RuntimeConfigDiffDialog from "@/components/profiles/runtime-config-diff-dialog";
import { filterProfiles } from "@/components/profiles/utils";
import { Public } from "@mui/icons-material";
import { Badge, Button, IconButton } from "@mui/material";
import Grid from "@mui/material/Grid2";
import { Profile, useClash } from "@nyanpasu/interface";
import { SidePage } from "@nyanpasu/ui";
import { createFileRoute, useLocation } from "@tanstack/react-router";
import { zodSearchValidator } from "@tanstack/router-zod-adapter";

const profileSearchParams = z.object({
  subscribeName: z.string().optional(),
  subscribeUrl: z.string().url().optional(),
  subscribeDesc: z.string().optional(),
});

export const Route = createFileRoute("/profiles")({
  validateSearch: zodSearchValidator(profileSearchParams),
  component: ProfilePage,
});

export default function ProfilePage() {
  const { t } = useTranslation();
  const { getProfiles, getRuntimeLogs } = useClash();
  const maxLogLevelTriggered = useMemo(() => {
    const currentProfileChains =
      getProfiles.data?.items?.find(
        (item) => item.uid == getProfiles.data?.current,
      )?.chains || [];
    return Object.entries(getRuntimeLogs.data || {}).reduce(
      (acc, [key, value]) => {
        const accKey = currentProfileChains.includes(key)
          ? "current"
          : "global";
        if (acc[accKey] == "error") {
          return acc;
        }
        for (const log of value) {
          switch (log[0]) {
            case "error":
              return { ...acc, [accKey]: "error" };
            case "warn":
              acc = { ...acc, [accKey]: "warn" };
              break;
            case "info":
              if (acc[accKey] != "warn") {
                acc = { ...acc, [accKey]: "info" };
              }
              break;
          }
        }
        return acc;
      },
      {} as {
        global: undefined | "info" | "error" | "warn";
        current: undefined | "info" | "error" | "warn";
      },
    );
  }, [getRuntimeLogs.data, getProfiles.data]);
  const { profiles } = filterProfiles(getProfiles.data?.items);

  const [globalChain, setGlobalChain] = useAtom(atomGlobalChainCurrent);

  const [chainsSelected, setChainsSelected] = useAtom(atomChainsSelected);

  const handleGlobalChainClick = () => {
    setChainsSelected(undefined);
    setGlobalChain(!globalChain);
  };

  const onClickChains = (profile: Profile.Item) => {
    setGlobalChain(false);

    if (chainsSelected == profile.uid) {
      setChainsSelected(undefined);
    } else {
      setChainsSelected(profile.uid);
    }
  };

  const handleSideClose = () => {
    setChainsSelected(undefined);
    setGlobalChain(false);
  };

  const [runtimeConfigViewerOpen, setRuntimeConfigViewerOpen] = useState(false);
  const location = useLocation();
  const addProfileCtxValue = useMemo(() => {
    if (!location.search || !location.search.subscribeUrl) {
      return null;
    }
    return {
      name: location.search.subscribeName!,
      desc: location.search.subscribeDesc!,
      url: location.search.subscribeUrl,
    } satisfies AddProfileContextValue;
  }, [location.search]);

  const hasSide = globalChain || chainsSelected;

  const { width } = useWindowSize();

  return (
    <SidePage
      title={t("Profiles")}
      flexReverse
      header={
        <div className="flex items-center gap-2">
          <RuntimeConfigDiffDialog
            open={runtimeConfigViewerOpen}
            onClose={() => setRuntimeConfigViewerOpen(false)}
          />
          <IconButton
            className="h-10 w-10"
            color="inherit"
            title="Runtime Config"
            onClick={() => {
              setRuntimeConfigViewerOpen(true);
            }}
          >
            <IconMdiTextBoxCheckOutline
            // style={{
            //   color: theme.palette.text.primary,
            // }}
            />
          </IconButton>
          <Badge
            variant="dot"
            color={
              maxLogLevelTriggered.global === "error"
                ? "error"
                : maxLogLevelTriggered.global === "warn"
                  ? "warning"
                  : "primary"
            }
            invisible={!maxLogLevelTriggered.global}
          >
            <Button
              size="small"
              variant={globalChain ? "contained" : "outlined"}
              onClick={handleGlobalChainClick}
              startIcon={<Public />}
            >
              {t("Global Proxy Chains")}
            </Button>
          </Badge>
        </div>
      }
      side={hasSide && <ProfileSide onClose={handleSideClose} />}
    >
      <AnimatePresence initial={false} mode="sync">
        <div className="flex flex-col gap-4 p-6">
          <QuickImport />

          {profiles && (
            <Grid container spacing={2}>
              {profiles.map((item) => (
                <Grid
                  key={item.uid}
                  size={{
                    xs: 12,
                    sm: 12,
                    md: hasSide && width <= 1000 ? 12 : 6,
                    lg: 4,
                    xl: 3,
                  }}
                >
                  <motion.div
                    key={item.uid}
                    layoutId={`profile-${item.uid}`}
                    layout="position"
                    initial={false}
                  >
                    <ProfileItem
                      item={item}
                      onClickChains={onClickChains}
                      selected={getProfiles.data?.current == item.uid}
                      maxLogLevelTriggered={maxLogLevelTriggered}
                      chainsSelected={chainsSelected == item.uid}
                    />
                  </motion.div>
                </Grid>
              ))}
            </Grid>
          )}
        </div>
      </AnimatePresence>

      <AddProfileContext.Provider value={addProfileCtxValue}>
        <NewProfileButton />
      </AddProfileContext.Provider>
    </SidePage>
  );
}
