/** @type {import('tailwindcss').Config} */
export default {
  content: ["./src/**/*.{tsx,ts}"],
  theme: {
    extend: {
      maxHeight: {
        "1/8": "calc(100vh / 8)",
      },
    },
  },
  plugins: [require("tailwindcss-textshadow")],
};
