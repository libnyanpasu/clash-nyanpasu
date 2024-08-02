import { useLockFn } from "ahooks";
import { useAtomValue } from "jotai";
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

  const currnetProfile = useAtomValue(atomChainsSelected);

  const { getProfiles, setProfilesConfig, setProfiles } = useClash();

  const { scripts } = filterProfiles(getProfiles.data?.items);

  const handleChainClick = useLockFn(async (uid: string) => {
    const chains = isGlobalChainCurrent
      ? (getProfiles.data?.chain ?? [])
      : (currnetProfile?.chains ?? []);

    const updatedChains = chains.includes(uid)
      ? chains.filter((chain) => chain !== uid)
      : [...chains, uid];

    try {
      if (isGlobalChainCurrent) {
        await setProfilesConfig({ chain: updatedChains });
      } else {
        await setProfiles(uid, { chains: updatedChains });
      }
    } catch (e) {
      message(`Apply error: ${formatError(e)}`, {
        type: "error",
        title: t("Error"),
      });
    }
  });

  return (
    <div className="h-full overflow-auto !pl-2 !pr-2">
      {scripts?.map((item, index) => {
        const selected = isGlobalChainCurrent
          ? getProfiles.data?.chain?.includes(item.uid)
          : currnetProfile?.chains?.includes(item.uid);

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

      <ListItemButton
        className="!mb-2 !mt-2 flex justify-center gap-2"
        sx={{
          backgroundColor: alpha(palette.secondary.main, 0.1),
          borderRadius: 4,
        }}
        onClick={() => onChainEdit()}
      >
        <Add color="primary" />

        <div className="py-1">New Chain</div>
      </ListItemButton>
    </div>
  );
};
