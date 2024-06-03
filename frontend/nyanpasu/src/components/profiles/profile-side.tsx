import { Close } from "@mui/icons-material";
import { IconButton } from "@mui/material";
import { Profile } from "@nyanpasu/interface";
import { useState } from "react";
import { ScriptDialog } from "./script-dialog";
import { SideLog } from "./modules/side-log";
import { Allotment } from "allotment";
import "allotment/dist/style.css";
import { SideChain } from "./modules/side-chain";

export interface ProfileSideProps {
  profile?: Profile.Item;
  global?: boolean;
  onClose: () => void;
}

export const ProfileSide = ({ profile, global, onClose }: ProfileSideProps) => {
  const [open, setOpen] = useState(false);

  const [item, setItem] = useState<Profile.Item>();

  const handleEditChain = async (item?: Profile.Item) => {
    setItem(item);
    setOpen(true);
  };

  return (
    <>
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

      <div style={{ height: "calc(100% - 84px)" }}>
        <Allotment vertical defaultSizes={[1, 0]}>
          <Allotment.Pane snap>
            <SideChain
              global={global}
              profile={profile}
              onChainEdit={handleEditChain}
            />
          </Allotment.Pane>

          <Allotment.Pane minSize={40}>
            <SideLog className="h-full" />
          </Allotment.Pane>
        </Allotment>
      </div>

      <ScriptDialog
        open={open}
        item={item}
        onClose={() => {
          setOpen(false);
          setItem(undefined);
        }}
      />
    </>
  );
};

export default ProfileSide;
