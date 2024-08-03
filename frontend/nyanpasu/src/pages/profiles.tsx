import { useAtom } from "jotai";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  atomChainsSelected,
  atomGlobalChainCurrent,
} from "@/components/profiles/modules/store";
import NewProfileButton from "@/components/profiles/new-profile-button";
import ProfileItem from "@/components/profiles/profile-item";
import ProfileSide from "@/components/profiles/profile-side";
import { QuickImport } from "@/components/profiles/quick-import";
import RuntimeConfigDiffDialog from "@/components/profiles/runtime-config-diff-dialog";
import { filterProfiles } from "@/components/profiles/utils";
import { Public } from "@mui/icons-material";
import Masonry from "@mui/lab/Masonry";
import { Button, IconButton } from "@mui/material";
import { Profile, useClash } from "@nyanpasu/interface";
import { SidePage } from "@nyanpasu/ui";

export const ProfilePage = () => {
  const { t } = useTranslation();

  const { getProfiles } = useClash();

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
  console.log(runtimeConfigViewerOpen);
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
          <Button
            size="small"
            variant={globalChain ? "contained" : "outlined"}
            onClick={handleGlobalChainClick}
            startIcon={<Public />}
          >
            {t("Global Proxy Chains")}
          </Button>
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
                  chainsSelected={chainsSelected == item.uid}
                />
              );
            })}
          </Masonry>
        )}
      </div>

      <NewProfileButton />
    </SidePage>
  );
};

export default ProfilePage;
