"use strict";
const config = require("conventional-changelog-conventionalcommits");

const GIT_COMMIT_WITH_AUTHOR_FORMAT =
  "%B%n-hash-%n%H%n-gitTags-%n%d%n-committerDate-%n%ci%n-authorName-%n%an%n-authorEmail-%n%ae%n-gpgStatus-%n%G?%n-gpgSigner-%n%GS";
const extraCommitMsg = `by {{authorName}}`;

const configs = config({
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

config.gitRawCommitsOpts.format = GIT_COMMIT_WITH_AUTHOR_FORMAT;
config.writerOpts.commitPartial =
  config.writerOpts.commitPartial.replace(/\n*$/, "") +
  ` {{#if @root.linkReferences~}}${extraCommitMsg}{{~/if}}\n`;

module.exports = configs;
