import { useAtom } from "jotai";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useLocation } from "react-router-dom";
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
import Masonry from "@mui/lab/Masonry";
import { Badge, Button, IconButton } from "@mui/material";
import { Profile, useClash } from "@nyanpasu/interface";
import { SidePage } from "@nyanpasu/ui";

export const ProfilePage = () => {
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
    if (!location.state || !location.state.subscribe) {
      return null;
    }
    return {
      name: location.state.subscribe.name,
      desc: location.state.subscribe.desc,
      url: location.state.subscribe.url,
    } satisfies AddProfileContextValue;
  }, [location.state]);

  return (
    <SidePage
      title={t("Profiles")}
      flexReverse
      header={
        <div>
          <RuntimeConfigDiffDialog
            open={runtimeConfigViewerOpen}
            onClose={() => setRuntimeConfigViewerOpen(false)}
          />
          <IconButton
            onClick={() => {
              setRuntimeConfigViewerOpen(true);
            }}
          >
            <IconMdiTextBoxCheckOutline />
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
      sideClassName="!overflow-visible"
      side={
        (globalChain || chainsSelected) && (
          <ProfileSide onClose={handleSideClose} />
        )
      }
    >
      <div className="flex flex-col gap-4 p-6">
        <QuickImport />

        {profiles && (
          <Masonry
            columns={{ xs: 1, sm: 1, md: 2, xl: 3 }}
            spacing={2}
            sx={{ width: "calc(100% + 24px)" }}
          >
            {profiles.map((item, index) => {
              return (
                <ProfileItem
                  key={index}
                  item={item}
                  onClickChains={onClickChains}
                  selected={getProfiles.data?.current == item.uid}
                  maxLogLevelTriggered={maxLogLevelTriggered}
                  chainsSelected={chainsSelected == item.uid}
                />
              );
            })}
          </Masonry>
        )}
      </div>
      <AddProfileContext.Provider value={addProfileCtxValue}>
        <NewProfileButton />
      </AddProfileContext.Provider>
    </SidePage>
  );
};

export default ProfilePage;
