import { useVerge } from "@/hooks/use-verge";
import { classNames } from "@/utils";
import { motion, type HTMLMotionProps } from "framer-motion";
import { useState } from "react";
import { useOutlet } from "react-router-dom";

type Props = {
  children?: React.ReactNode;
};

interface PageTransitionVariant {
  initial: HTMLMotionProps<"div">["initial"];
  visible: HTMLMotionProps<"div">["animate"];
  hidden: HTMLMotionProps<"div">["exit"];
  transition?: HTMLMotionProps<"div">["transition"];
}

const commonTransition = {
  type: "spring",
  bounce: 0.3,
  duration: 0.5,
  delayChildren: 0.2,
  staggerChildren: 0.05,
};

export const pageTransitionVariants = {
  blur: {
    initial: { opacity: 0, filter: "blur(10px)" },
    visible: { opacity: 1, filter: "blur(0px)" },
    hidden: { opacity: 0, filter: "blur(10px)" },
  },
  slide: {
    initial: {
      translateY: "50%",
      opacity: 0,
      scale: 0.9,
    },
    visible: {
      translateY: "0%",
      opacity: 1,
      scale: 1,
      transition: commonTransition,
    },
    hidden: {
      translateY: "-50%",
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
  none: {
    initial: {},
    visible: {},
    hidden: {},
  },
} satisfies Record<string, PageTransitionVariant>;

function overrideVariantsTransition(
  variants: Record<string, PageTransitionVariant>,
  transition?: HTMLMotionProps<"div">["transition"],
) {
  if (!transition) return variants;
  return Object.keys(variants).reduce(
    (acc, cur) => {
      acc[cur] = Object.entries(variants[cur]).reduce((acc, [key, value]) => {
        if (key === "initial") {
          acc[key] = value;
          return acc;
        }
        // @ts-expect-error ts(7053) - 懒得针对工具方法做类型体操了
        acc[key] = {
          ...value,
          transition,
        };
        return acc;
      }, {} as PageTransitionVariant);
      return acc;
    },
    {} as Record<string, PageTransitionVariant>,
  );
}

const AnimatedOutlet: React.FC = () => {
  const o = useOutlet();
  const [outlet] = useState(o);

  return <>{outlet}</>;
};

export default function PageTransition() {
  const { verge } = useVerge();
  const { theme_setting } = verge ?? {};
  const variants = overrideVariantsTransition(
    pageTransitionVariants,
    theme_setting?.page_transition_duration
      ? {
          duration: theme_setting.page_transition_duration,
        }
      : undefined,
  ) as typeof pageTransitionVariants;
  return (
    <motion.div
      className={classNames("page-transition", "the-content")}
      key={location.pathname}
      variants={variants[verge?.page_transition_animation ?? "slide"]}
      initial="initial"
      animate="visible"
      exit="hidden"
    >
      <AnimatedOutlet />
    </motion.div>
  );
}
