import { classNames } from "@/utils";
import { useNyanpasu } from "@nyanpasu/interface";
import { AnimatePresence, Variant, motion } from "framer-motion";
import { useLocation, useOutlet } from "react-router-dom";

type PageVariantKey = "initial" | "visible" | "hidden";

type PageVariant = {
  [key in PageVariantKey]: Variant;
};

const commonTransition = {
  type: "spring",
  bounce: 0,
  duration: 0.35,
};

export const pageTransitionVariants: { [name: string]: PageVariant } = {
  blur: {
    initial: { opacity: 0, filter: "blur(10px)" },
    visible: { opacity: 1, filter: "blur(0px)" },
    hidden: { opacity: 0, filter: "blur(10px)" },
  },
  slide: {
    initial: {
      translateY: "30%",
      opacity: 0,
      scale: 0.95,
    },
    visible: {
      translateY: "0%",
      opacity: 1,
      scale: 1,
      transition: commonTransition,
    },
    hidden: {
      opacity: 0,
      scale: 0.9,
      transition: commonTransition,
    },
  },
  transparent: {
    initial: { opacity: 0 },
    visible: { opacity: 1 },
    hidden: { opacity: 0 },
  },
};

export default function PageTransition({ className }: { className?: string }) {
  const { nyanpasuConfig } = useNyanpasu();

  const outlet = useOutlet();

  const hashkey = useLocation().pathname;

  const variants = nyanpasuConfig?.lighten_animation_effects
    ? pageTransitionVariants.transparent
    : pageTransitionVariants.slide;

  return (
    <AnimatePresence mode="popLayout" initial={false}>
      <motion.div
        className={classNames("page-transition", className)}
        key={hashkey}
        layout
        layoutId={hashkey}
        variants={variants}
        initial="initial"
        animate="visible"
        exit="hidden"
      >
        {outlet}
      </motion.div>
    </AnimatePresence>
  );
}
