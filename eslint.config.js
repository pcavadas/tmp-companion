import globals from "globals";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import react from "eslint-plugin-react";
import eslintConfigPrettier from "eslint-config-prettier";

export default tseslint.config(
  {
    ignores: [
      "dist/",
      "src-tauri/",
      "node_modules/",
      ".design-ref/",
      // The e2e browser shim is hand-written JS that pokes at Tauri's window internals —
      // it isn't in any TS project, so type-aware linting can't apply to it.
      "e2e/bridge-client.js",
    ],
  },

  tseslint.configs.eslintRecommended,
  // Production-grade, type-aware linting: the STRICTEST typescript-eslint presets.
  // strict-type-checked is a superset of recommended-type-checked (no-non-null-
  // assertion, no-unnecessary-condition, no-confusing-void-expression, …);
  // stylistic-type-checked adds consistency rules. Both require the parser to load
  // type information — see parserOptions.projectService below.
  ...tseslint.configs.strictTypeChecked,
  ...tseslint.configs.stylisticTypeChecked,
  react.configs.flat.recommended,
  react.configs.flat["jsx-runtime"],

  {
    files: ["src/**/*.{ts,tsx}"],
    languageOptions: {
      ecmaVersion: "latest",
      globals: globals.browser,
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
    settings: {
      react: { version: "detect" },
    },
    plugins: {
      "react-hooks": reactHooks,
      "react-refresh": reactRefresh,
    },
    rules: {
      ...reactHooks.configs["recommended-latest"].rules,
      // Superseded by TypeScript's own type system — props are validated by the
      // compiler, so the runtime prop-types check is redundant (not silenced).
      // (react-in-jsx-scope / jsx-uses-react are likewise off via jsx-runtime
      // above, as the React 19 automatic JSX transform requires.)
      "react/prop-types": "off",
      "react-refresh/only-export-components": [
        "warn",
        { allowConstantExport: true },
      ],
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
      ],
    },
  },

  {
    files: ["*.config.{js,ts}", "vite.config.ts", "vitest.config.ts"],
    languageOptions: {
      globals: globals.node,
    },
  },

  // The Playwright e2e harness (specs + fixtures) — type-aware linting under the same strict
  // presets as src/. Browser globals (the page-context spec callbacks) + node globals.
  {
    files: ["e2e/**/*.{ts,tsx}"],
    languageOptions: {
      globals: { ...globals.browser, ...globals.node },
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
    settings: {
      react: { version: "detect" },
    },
  },

  eslintConfigPrettier,
);
