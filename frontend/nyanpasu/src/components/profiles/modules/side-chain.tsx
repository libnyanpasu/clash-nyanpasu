import { useLockFn } from "ahooks";
import { Reorder } from "framer-motion";
import { useAtomValue } from "jotai";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { formatError } from "@/utils";
import { message } from "@/utils/notification";
import { Add } from "@mui/icons-material";
import { alpha, ListItemButton, useTheme } from "@mui/material";
import { Profile, useClash } from "@nyanpasu/interface";
import { filterProfiles } from "../utils";
import ChainItem from "./chain-item";
import { atomChainsSelected, atomGlobalChainCurrent } from "./store";

export interface SideChainProps {
  onChainEdit: (item?: Profile.Item) => void | Promise<void>;
}

export const SideChain = ({ onChainEdit }: SideChainProps) => {
  const { t } = useTranslation();

  const { palette } = useTheme();

  const isGlobalChainCurrent = useAtomValue(atomGlobalChainCurrent);

  const currentProfileUid = useAtomValue(atomChainsSelected);

  const { getProfiles, setProfilesConfig, setProfiles, reorderProfilesByList } =
    useClash();

  const { scripts, profiles } = filterProfiles(getProfiles.data?.items);

  const currentProfile = useMemo(() => {
    return getProfiles.data?.items?.find(
      (item) => item.uid === currentProfileUid,
    );
  }, [getProfiles.data?.items, currentProfileUid]);

  const handleChainClick = useLockFn(async (uid: string) => {
    const chains = isGlobalChainCurrent
      ? (getProfiles.data?.chain ?? [])
      : (currentProfile?.chain ?? []);

    const updatedChains = chains.includes(uid)
      ? chains.filter((chain) => chain !== uid)
      : [...chains, uid];

    try {
      if (isGlobalChainCurrent) {
        await setProfilesConfig({ chain: updatedChains });
      } else {
        if (!currentProfile?.uid) {
          return;
        }
        await setProfiles(currentProfile!.uid, { chain: updatedChains });
      }
    } catch (e) {
      message(`Apply error: ${formatError(e)}`, {
        kind: "error",
        title: t("Error"),
      });
    }
  });

  const reorderValues = useMemo(
    () => scripts?.map((item) => item.uid) || [],
    [scripts],
  );

  return (
    <div className="h-full overflow-auto !pl-2 !pr-2">
      <Reorder.Group
        axis="y"
        values={reorderValues}
        onReorder={(values) => {
          const profileUids = profiles?.map((item) => item.uid) || [];
          reorderProfilesByList([...profileUids, ...values]);
        }}
        layoutScroll
        style={{ overflowY: "scroll" }}
      >
        {scripts?.map((item, index) => {
          const selected = isGlobalChainCurrent
            ? getProfiles.data?.chain?.includes(item.uid)
            : currentProfile?.chain?.includes(item.uid);

          return (
            <ChainItem
              key={index}
              item={item}
              selected={selected}
              onClick={async () => await handleChainClick(item.uid)}
              onChainEdit={() => onChainEdit(item)}
            />
          );
        })}
      </Reorder.Group>

      <ListItemButton
        className="!mb-2 !mt-2 flex justify-center gap-2"
        sx={{
          backgroundColor: alpha(palette.secondary.main, 0.1),
          borderRadius: 4,
        }}
        onClick={() => onChainEdit()}
      >
        <Add color="primary" />

        <div className="py-1">{t("New Chain")}</div>
      </ListItemButton>
    </div>
  );
};
