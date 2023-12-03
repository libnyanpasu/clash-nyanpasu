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

export default function PageTransition({ children }: Props) {
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
      variants={variants[verge?.page_transition_animation ?? "slide"]}
      initial="initial"
      animate="visible"
      exit="hidden"
    >
      {children}
    </motion.div>
  );
}
