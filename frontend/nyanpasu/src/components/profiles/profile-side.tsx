import { Allotment } from "allotment";
import "allotment/dist/style.css";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Close } from "@mui/icons-material";
import { IconButton } from "@mui/material";
import { Profile } from "@nyanpasu/interface";
import { SideChain } from "./modules/side-chain";
import { SideLog } from "./modules/side-log";
import { ScriptDialog } from "./script-dialog";

export interface ProfileSideProps {
  profile?: Profile.Item;
  global?: boolean;
  onClose: () => void;
}

export const ProfileSide = ({ profile, global, onClose }: ProfileSideProps) => {
  const { t } = useTranslation();

  const [open, setOpen] = useState(false);

  const [item, setItem] = useState<Profile.Item>();

  const handleEditChain = async (item?: Profile.Item) => {
    setItem(item);
    setOpen(true);
  };

  return (
    <>
      <div className="flex items-start justify-between p-4 pr-2">
        <div>
          <div className="text-xl font-bold">{t("Proxy Chains")}</div>

          <div className="truncate">
            {global ? t("Global Proxy Chains") : profile?.name}
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
            <SideLog className="h-full border-t-2" />
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
