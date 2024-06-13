import LogoSvg from "@/assets/image/logo.svg?react";
import { AnimatePresence, Variants, motion } from "framer-motion";
import { classNames } from "@/utils";
import { CSSProperties } from "react";
import styles from "./animated-logo.module.scss";
import { useNyanpasu } from "@nyanpasu/interface";

const Logo = motion(LogoSvg);

const transition = {
  type: "spring",
  stiffness: 260,
  damping: 20,
};

const motionVariants: { [name: string]: Variants } = {
  default: {
    initial: {
      opacity: 0,
      scale: 0.5,
      transition,
    },
    animate: {
      opacity: 1,
      scale: 1,
      transition,
    },
    exit: {
      opacity: 0,
      scale: 0.5,
      transition,
    },
    whileHover: {
      scale: 1.1,
      transition,
    },
  },
  none: {
    initial: {},
    animate: {},
    exit: {},
  },
};

export default function AnimatedLogo({
  className,
  style,
  disbaleMotion,
}: {
  className?: string;
  style?: CSSProperties;
  disbaleMotion?: boolean;
}) {
  const { nyanpasuConfig } = useNyanpasu();

  const disbale = disbaleMotion ?? nyanpasuConfig?.lighten_animation_effects;

  return (
    <AnimatePresence initial={false}>
      <Logo
        className={classNames(styles.LogoSchema, className)}
        variants={motionVariants[disbale ? "none" : "default"]}
        style={style}
      />
    </AnimatePresence>
  );
}
