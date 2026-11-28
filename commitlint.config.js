// Conventional Commits, checked on pull request titles and commits (.github/workflows/pr-title.yml,
// commits.yml). Pull requests are squash-merged: the title feeds the changelog and the version bump.
export default {
  extends: ["@commitlint/config-conventional"],
};
