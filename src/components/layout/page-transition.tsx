import { classNames } from "@/utils";
import { motion, type HTMLMotionProps } from "framer-motion";

type Props = {
  children?: React.ReactNode;
};

interface PageTransitionVariant {
  initial: HTMLMotionProps<"div">["initial"];
  animate: HTMLMotionProps<"div">["animate"];
  exit: HTMLMotionProps<"div">["exit"];
  transition?: HTMLMotionProps<"div">["transition"];
}

const variants = {
  blur: {
    initial: { opacity: 0, filter: "blur(10px)" },
    animate: { opacity: 1, filter: "blur(0px)" },
    exit: { opacity: 0, filter: "blur(10px)" },
  },
  slide: {
    initial: { translateY: "50%", opacity: 0, scale: 0.9 },
    animate: { translateY: "0%", opacity: 1, scale: 1 },
    exit: { translateY: "-50%", opacity: 0, scale: 0.9 },
  },
} satisfies Record<string, PageTransitionVariant>;

export default function PageTransition({ children }: Props) {
  return (
    <motion.div
      className={classNames("page-transition", "the-content")}
      key={location.pathname}
      variants={variants.slide}
      initial="initial"
      animate="animate"
      exit="exit"
      transition={{
        duration: 0.3,
      }}
    >
      {children}
    </motion.div>
  );
}
