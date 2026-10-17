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
    // No escape hatches in src/. `any` and non-null `!` are already errors via
    // strictTypeChecked; these two close the remaining holes so the rule is
    // enforced by the linter rather than by prose someone has to have read:
    //   - noInlineConfig makes an `eslint-disable` comment unable to silence
    //     anything, and reportUnusedDisableDirectives then flags it as an error.
    //   - ban-ts-comment below rejects @ts-expect-error too, which the preset
    //     default would otherwise allow when it carries a description.
    // Fix findings by changing code, never by silencing.
    linterOptions: {
      noInlineConfig: true,
      reportUnusedDisableDirectives: "error",
    },
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
      // The preset default permits @ts-expect-error when it has a description;
      // this repo permits none of the three.
      "@typescript-eslint/ban-ts-comment": [
        "error",
        {
          "ts-expect-error": true,
          "ts-ignore": true,
          "ts-nocheck": true,
          "ts-check": false,
        },
      ],
    },
  },

  {
    files: ["*.config.{js,ts}", "vite.config.ts", "vitest.config.ts"],
    languageOptions: {
      globals: globals.node,
    },
  },
  // Root-level config files aren't part of the tsconfig `include` (src/e2e only), so they
  // have no type info for the type-checked presets above — drop just those rules, keep the rest.
  {
    files: ["*.config.{js,ts}", "vite.config.ts", "vitest.config.ts"],
    ...tseslint.configs.disableTypeChecked,
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

  {
    // process.env.TMP_E2E_ONLINE is no mode signal in the Playwright test
    // process — each config pins it on its own e2e_server — so a spec/fixture
    // that reads it directly silently takes the offline branch even online. HW-reproduced twice: a since-removed Doctor apply spec's original
    // describe-level `test.skip(!process.env.TMP_E2E_ONLINE)` (always true) and
    // `ensureScenario` (`e2e/fixtures/scenario.ts`) gating its online/offline
    // check the same way, which silently skipped `e2e_seed_scenario` online and
    // let a mutilated preset pass the (offline) presence-only check. The correct
    // pattern — ask the server via its `/health` endpoint — already lives in the
    // same file as `isOnline(page)`, used by `clearScenario`.
    files: ["e2e/**/*.{ts,tsx}"],
    rules: {
      "no-restricted-syntax": [
        "error",
        {
          // Dot form: process.env.TMP_E2E_ONLINE — the form both historical
          // occurrences used.
          selector:
            "MemberExpression[object.object.name='process'][object.property.name='env'][property.name='TMP_E2E_ONLINE']",
          message:
            "process.env.TMP_E2E_ONLINE is no mode signal in the Playwright test process (each config pins it on its own e2e_server) — use `await isOnline(page)` from e2e/fixtures/scenario.ts instead.",
        },
        {
          // Bracket form: process.env["TMP_E2E_ONLINE"] — same env read, computed
          // access, so `property.name` above doesn't match it.
          selector:
            "MemberExpression[computed=true][object.object.name='process'][object.property.name='env'][property.value='TMP_E2E_ONLINE']",
          message:
            "process.env.TMP_E2E_ONLINE is no mode signal in the Playwright test process (each config pins it on its own e2e_server) — use `await isOnline(page)` from e2e/fixtures/scenario.ts instead.",
        },
      ],
    },
  },

  {
    // Error text on screen goes through `errMsg` (src/lib/format.ts), never a
    // hand-rolled `e instanceof Error ? e.message : "generic"`: a command
    // rejection used to arrive as a bare string, so that idiom showed the generic
    // fallback and threw the backend's real reason away (the old Doctor fix cards
    // always read "Couldn't apply this fix."). `invoke` now wraps rejections in
    // `CommandError`, and `errMsg` handles every shape a `catch` can receive.
    files: ["src/views/**/*.{ts,tsx}", "src/App.tsx"],
    rules: {
      "no-restricted-syntax": [
        "error",
        {
          selector:
            "BinaryExpression[operator='instanceof'][right.name='Error']",
          message:
            "Render a caught value with `errMsg(e)` from src/lib/format.ts — never a hand-rolled `instanceof Error` check.",
        },
      ],
    },
  },

  {
    // The block-art modules must NOT import catalog.ts or cpu.ts (catalog imports
    // cpu). That edge once closed the module-init cycle blockArt → catalog → cpu →
    // blockArt (a TDZ "cannot access before initialization" crash), and it would
    // pull the catalog into every art lookup. The id-suffix rule all three need
    // lives in the zero-import `./deviceId` leaf instead. Cross-cutting form+art
    // decisions resolve at the view call site (which may import both), never
    // inside a core model module. (No import plugin is a dependency, so this is an
    // explicit pattern rather than `import/no-cycle`.)
    files: ["src/models/blockArt.ts", "src/models/blockArtCatalog/**/*.ts"],
    rules: {
      "@typescript-eslint/no-restricted-imports": [
        "error",
        {
          patterns: [
            {
              regex: "(^|/)(catalog|cpu)(\\.ts)?$",
              message:
                "block-art modules must not import catalog or cpu — that closes the blockArt→catalog→cpu→blockArt module-init cycle (a TDZ crash). Import the id rule from ./deviceId; resolve form+art at the view call site.",
            },
          ],
        },
      ],
    },
  },
  {
    // deviceId.ts is the zero-import leaf cpu, catalog and blockArt all share — any
    // import here can reintroduce the cycle it exists to break.
    files: ["src/models/deviceId.ts"],
    rules: {
      "no-restricted-imports": [
        "error",
        {
          patterns: [
            {
              regex: ".",
              message:
                "deviceId.ts must stay a zero-import leaf (cpu, catalog and blockArt all import it).",
            },
          ],
        },
      ],
    },
  },

  eslintConfigPrettier,
);
