"use strict";
const config = require("conventional-changelog-conventionalcommits");

module.exports = config({
  types: [
    {
      type: "feat",
      section: "✨ Features",
    },
    {
      type: "fix",
      section: "🐛 Bug Fixes",
    },
    {
      type: "chore",
      section: "🧹 Maintenance",
    },
    {
      type: "docs",
      section: "📚 Documentation",
    },
    {
      type: "style",
      section: "💅 Styles",
    },
    {
      type: "refactor",
      section: "🔨 Refactoring",
    },
    {
      type: "perf",
      section: "⚡ Performance Improvements",
    },
    {
      type: "test",
      section: "✅ Tests",
    },
  ],
});
