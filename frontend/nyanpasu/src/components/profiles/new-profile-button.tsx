import { Add } from "@mui/icons-material";
import { FloatingButton } from "@nyanpasu/ui";
import { useState } from "react";
import { ProfileDialog } from "./profile-dialog";

export const NewProfileButton = () => {
  const [open, setOpen] = useState(false);

  return (
    <>
      <FloatingButton onClick={() => setOpen(true)}>
        <Add className="!size-8 absolute" />
      </FloatingButton>

      <ProfileDialog open={open} onClose={() => setOpen(false)} />
    </>
  );
};

export default NewProfileButton;
