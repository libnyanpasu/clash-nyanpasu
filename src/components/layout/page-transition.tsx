import { useVerge } from "@/hooks/use-verge";
import { classNames } from "@/utils";
import { motion, type HTMLMotionProps } from "framer-motion";

type Props = {
  children?: React.ReactNode;
};

interface PageTransitionVariant {
  initial: HTMLMotionProps<"div">["initial"];
  visible: HTMLMotionProps<"div">["animate"];
  hidden: HTMLMotionProps<"div">["exit"];
  transition?: HTMLMotionProps<"div">["transition"];
}

export const pageTransitionVariants = {
  blur: {
    initial: { opacity: 0, filter: "blur(10px)" },
    visible: { opacity: 1, filter: "blur(0px)" },
    hidden: { opacity: 0, filter: "blur(10px)" },
  },
  slide: {
    initial: { translateY: "50%", opacity: 0, scale: 0.9 },
    visible: {
      translateY: "0%",
      opacity: 1,
      scale: 1,
      transition: { duration: 0.15 },
    },
    hidden: {
      translateY: "-50%",
      opacity: 0,
      scale: 0.9,
      transition: { duration: 0.15 },
    },
  },
  transparent: {
    initial: { opacity: 0 },
    visible: { opacity: 1 },
    hidden: { opacity: 0 },
  },
  none: {
    initial: {},
    visible: {},
    hidden: {},
  },
} satisfies Record<string, PageTransitionVariant>;

export default function PageTransition({ children }: Props) {
  const { verge } = useVerge();
  return (
    <motion.div
      className={classNames("page-transition", "the-content")}
      variants={
        pageTransitionVariants[verge?.page_transition_animation ?? "slide"]
      }
      initial="initial"
      animate="visible"
      exit="hidden"
    >
      {children}
    </motion.div>
  );
}
