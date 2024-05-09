import LogoSvg from "@/assets/image/logo.svg?react";
import getSystem from "@/utils/get-system";
import { motion } from "framer-motion";
import { useRef } from "react";
import { UpdateButton } from "./update-button";

const OS = getSystem();

const Logo = motion(LogoSvg);

export default function AnimatedLogo() {
  const constraintsRef = useRef<HTMLDivElement>(null);

  return (
    <div className="the-logo" data-windrag ref={constraintsRef}>
      <Logo
        initial={{ opacity: 0, scale: 0.5 }}
        animate={{ opacity: 1, scale: 1 }}
        whileHover={{ scale: 1.1 }}
        transition={{
          type: "spring",
          stiffness: 260,
          damping: 20,
        }}
        drag
        dragConstraints={constraintsRef}
      />

      {!(OS === "windows" && WIN_PORTABLE) && (
        <UpdateButton className="the-newbtn" />
      )}
    </div>
  );
}
