import { Button } from "@mui/material";
import { Profile, useClash } from "@nyanpasu/interface";
import { SidePage } from "@nyanpasu/ui";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import ProfileItem from "@/components/profiles/profile-item";
import ProfileSide from "@/components/profiles/profile-side";
import { filterProfiles } from "@/components/profiles/utils";
import NewProfileButton from "@/components/profiles/new-profile-button";
import { QuickImport } from "@/components/profiles/quick-import";
import Masonry from "@mui/lab/Masonry";
import { Public } from "@mui/icons-material";

export const ProfilePage = () => {
  const { t } = useTranslation();

  const { getProfiles } = useClash();

  const { profiles } = filterProfiles(getProfiles.data?.items);

  const [globalChain, setGlobalChain] = useState(false);

  const handleGlobalChainClick = () => {
    setChainsSelected(undefined);
    setGlobalChain(!globalChain);
  };

  const [chainsSelected, setChainsSelected] = useState<Profile.Item>();

  const onClickChains = (profile: Profile.Item) => {
    setGlobalChain(false);

    if (chainsSelected?.uid == profile.uid) {
      setChainsSelected(undefined);
    } else {
      setChainsSelected(profile);
    }
  };

  const handleSideClose = () => {
    setChainsSelected(undefined);
    setGlobalChain(false);
  };

  return (
    <SidePage
      title={t("Profiles")}
      flexReverse
      header={
        <div>
          <Button
            size="small"
            variant={globalChain ? "contained" : "outlined"}
            onClick={handleGlobalChainClick}
            startIcon={<Public />}
          >
            Global Chain
          </Button>
        </div>
      }
      sideClassName="!overflow-visible"
      side={
        (globalChain || chainsSelected) && (
          <ProfileSide
            profile={chainsSelected}
            global={globalChain}
            onClose={handleSideClose}
          />
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
                  chainsSelected={chainsSelected?.uid == item.uid}
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
