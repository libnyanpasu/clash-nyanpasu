import { use, useEffect, useState } from "react";
import { Add } from "@mui/icons-material";
import { FloatingButton } from "@nyanpasu/ui";
import { AddProfileContext, ProfileDialog } from "./profile-dialog";

export const NewProfileButton = () => {
  const addProfileCtx = use(AddProfileContext);
  const [open, setOpen] = useState(!!addProfileCtx);
  useEffect(() => {
    setOpen(!!addProfileCtx);
  }, [addProfileCtx]);
  return (
    <>
      <FloatingButton onClick={() => setOpen(true)}>
        <Add className="absolute !size-8" />
      </FloatingButton>

      <ProfileDialog open={open} onClose={() => setOpen(false)} />
    </>
  );
};

export default NewProfileButton;
