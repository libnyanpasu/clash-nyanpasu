import LogoSvg from "@/assets/image/logo.svg?react";
import { motion } from "framer-motion";
import { classNames } from "@/utils";
import { CSSProperties } from "react";
import styles from "./animated-logo.module.scss";

const Logo = motion(LogoSvg);

export default function AnimatedLogo({
  className,
  style,
}: {
  className?: string;
  style?: CSSProperties;
}) {
  return (
    <Logo
      className={classNames(styles.LogoSchema, className)}
      initial={{ opacity: 0, scale: 0.5 }}
      animate={{ opacity: 1, scale: 1 }}
      whileHover={{ scale: 1.1 }}
      transition={{
        type: "spring",
        stiffness: 260,
        damping: 20,
      }}
      style={style}
    />
  );
}
