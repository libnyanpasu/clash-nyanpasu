import { Edit, Add } from "@mui/icons-material";
import {
  ListItemButton,
  alpha,
  ListItemText,
  IconButton,
  ListItemIcon,
  useTheme,
} from "@mui/material";
import { Profile, useClash } from "@nyanpasu/interface";
import { filterProfiles } from "../utils";
import { useLockFn } from "ahooks";
import { memo } from "react";

const ChainItem = memo(function ChainItem({
  name,
  desc,
  selected,
  onClick,
  onChainEdit,
}: {
  name?: string;
  desc?: string;
  selected?: boolean;
  onClick: () => void;
  onChainEdit: () => void;
}) {
  const { palette } = useTheme();

  return (
    <ListItemButton
      className="!mt-2 !mb-2"
      sx={{
        backgroundColor: selected
          ? alpha(palette.primary.main, 0.3)
          : alpha(palette.secondary.main, 0.1),
        borderRadius: 4,

        "&:hover": {
          backgroundColor: selected
            ? alpha(palette.primary.main, 0.5)
            : undefined,
        },
      }}
      onClick={onClick}
    >
      <ListItemText primary={name} secondary={desc} />

      <IconButton
        edge="end"
        color="primary"
        onClick={(e) => {
          e.preventDefault();
          e.stopPropagation();
          onChainEdit();
        }}
      >
        <Edit />
      </IconButton>
    </ListItemButton>
  );
});

export interface SideChainProps {
  global?: boolean;
  profile?: Profile.Item;
  onChainEdit: (item?: Profile.Item) => void | Promise<void>;
}

export const SideChain = ({ global, profile, onChainEdit }: SideChainProps) => {
  const { palette } = useTheme();

  const { getProfiles, setProfilesConfig, setProfiles } = useClash();

  const { scripts } = filterProfiles(getProfiles.data?.items);

  const handleChainClick = useLockFn(async (uid: string) => {
    const chains = global
      ? getProfiles.data?.chain ?? []
      : profile?.chains ?? [];

    const updatedChains = chains.includes(uid)
      ? chains.filter((chain) => chain !== uid)
      : [...chains, uid];

    if (global) {
      await setProfilesConfig({ chain: updatedChains });
    } else {
      await setProfiles(uid, { chains: updatedChains });
    }
  });

  return (
    <div className="!pl-2 !pr-2 overflow-auto h-full">
      {scripts?.map((item, index) => {
        const selected = global
          ? getProfiles.data?.chain?.includes(item.uid)
          : profile?.chains?.includes(item.uid);

        return (
          <ChainItem
            key={index}
            name={item.name}
            desc={item.desc}
            selected={selected}
            onClick={() => handleChainClick(item.uid)}
            onChainEdit={() => onChainEdit(item)}
          />
        );
      })}

      <ListItemButton
        className="!mt-2 !mb-2"
        sx={{
          backgroundColor: alpha(palette.secondary.main, 0.1),
          borderRadius: 4,
        }}
        onClick={() => onChainEdit()}
      >
        <ListItemIcon>
          <Add color="primary" />
        </ListItemIcon>

        <ListItemText primary="New Chain" />
      </ListItemButton>
    </div>
  );
};
