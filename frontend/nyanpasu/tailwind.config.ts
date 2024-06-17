import type { Config } from "tailwindcss";
import plugin from "tailwindcss/plugin";

export default {
  content: ["./src/**/*.{tsx,ts}"],
  theme: {
    extend: {
      maxHeight: {
        "1/8": "calc(100vh / 8)",
      },
    },
  },
  plugins: [
    require("tailwindcss-textshadow"),
    plugin(({ addBase }) => {
      addBase({
        ".scrollbar-hidden::-webkit-scrollbar": {
          width: "0px",
        },
      });
    }),
  ],
} as Config;
