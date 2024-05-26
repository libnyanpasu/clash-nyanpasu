import { Add, Close, Edit, RamenDining, Terminal } from "@mui/icons-material";
import {
  Divider,
  ListItemButton,
  ListItemIcon,
  ListItemText,
  IconButton,
  List,
  useTheme,
  alpha,
} from "@mui/material";
import { Profile, useClash } from "@nyanpasu/interface";
import { useState } from "react";
import { filterProfiles } from "./utils";
import { useLockFn } from "ahooks";
import { Expand, ExpandMore } from "@nyanpasu/ui";
import { isEmpty } from "lodash-es";
import { ScriptDialog } from "./script-dialog";
import { VList } from "virtua";

export interface ProfileSideProps {
  profile?: Profile.Item;
  global?: boolean;
  onClose: () => void;
}

export const ProfileSide = ({ profile, global, onClose }: ProfileSideProps) => {
  const { palette } = useTheme();

  const [open, setOpen] = useState(false);

  const { getProfiles, setProfilesConfig, getRuntimeLogs, setProfiles } =
    useClash();

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

  const [item, setItem] = useState<Profile.Item>();

  const handleEditChain = async (item: Profile.Item) => {
    setItem(item);
    setOpen(true);
  };

  const [expand, setExpand] = useState(false);

  return (
    <div className="relative h-full">
      <div className="flex-col gap-2">
        <div className="p-4 pr-2 flex justify-between items-start">
          <div>
            <div className="text-xl font-bold">Proxy Chains</div>

            <div className="truncate">
              {global ? "Global Chain" : profile?.name}
            </div>
          </div>

          <IconButton onClick={onClose}>
            <Close />
          </IconButton>
        </div>

        <List className="!pl-2 !pr-2 overflow-auto" disablePadding>
          {scripts?.map((item, index) => {
            const selected = global
              ? getProfiles.data?.chain?.includes(item.uid)
              : profile?.chains?.includes(item.uid);

            return (
              <ListItemButton
                key={index}
                className="!mt-2 !mb-2"
                sx={{
                  backgroundColor: selected
                    ? alpha(palette.primary.main, 0.3)
                    : alpha(palette.grey[100], 0.1),
                  borderRadius: 4,

                  "&:hover": {
                    backgroundColor: selected
                      ? alpha(palette.primary.main, 0.5)
                      : undefined,
                  },
                }}
                onClick={() => handleChainClick(item.uid)}
              >
                <ListItemText primary={item.name} secondary={item.desc} />

                <IconButton
                  edge="end"
                  color="primary"
                  onClick={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                    handleEditChain(item);
                  }}
                >
                  <Edit />
                </IconButton>
              </ListItemButton>
            );
          })}

          <ListItemButton
            className="!mt-2!mb-2"
            sx={{
              backgroundColor: alpha(palette.grey[100], 0.1),
              borderRadius: 4,
            }}
            onClick={() => setOpen(true)}
          >
            <ListItemIcon>
              <Add color="primary" />
            </ListItemIcon>

            <ListItemText primary="New Chian" />
          </ListItemButton>
        </List>
      </div>

      <ScriptDialog
        open={open}
        item={item}
        onClose={() => {
          setOpen(false);
          setItem(undefined);
        }}
      />

      <div className="absolute bottom-0 z-10 w-full">
        <Divider />

        <div className="p-1 pl-4 flex justify-between items-center">
          <div className="flex items-center gap-2">
            <Terminal />

            <span>Console</span>
          </div>

          <ExpandMore
            size="small"
            reverse
            expand={expand}
            onClick={() => setExpand(!expand)}
          />
        </div>

        <Expand open={expand}>
          <Divider />

          <VList className="flex flex-col gap-2 p-2 overflow-auto min-h-48 max-h-48">
            {!isEmpty(getRuntimeLogs.data) ? (
              Object.entries(getRuntimeLogs.data).map(([uid, content]) => {
                return content.map((item, index) => {
                  const name = scripts?.find(
                    (script) => script.uid === uid,
                  )?.name;

                  return (
                    <>
                      {index !== 0 && <Divider />}

                      <div key={uid + index} className="w-full font-mono">
                        <span className="text-red-500">[{name}]: </span>
                        <span>{item}</span>
                      </div>
                    </>
                  );
                });
              })
            ) : (
              <div className="w-full h-full min-h-48 flex flex-col justify-center items-center">
                <RamenDining className="!size-10" />
                <p>No Log</p>
              </div>
            )}
          </VList>
        </Expand>
      </div>
    </div>
  );
};

export default ProfileSide;
