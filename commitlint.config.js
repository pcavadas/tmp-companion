export default {
  extends: ["@commitlint/config-conventional"],
  // Known limit: dependabot's auto body (changelog/compare links) always exceeds
  // body-max-line-length; its headline is already conventional-commit valid.
  ignores: [(commit) => commit.includes("Signed-off-by: dependabot[bot]")],
};
