/** @format */

import js from "@eslint/js";
import globals from "globals";
import tseslint from "typescript-eslint";
import react from "eslint-plugin-react";
import reactHooks from "eslint-plugin-react-hooks";
import css from "@eslint/css";
import { defineConfig } from "eslint/config";

export default defineConfig([
  {
    ignores: [
      ".claude/**",
      ".ds-sync/**",
      "dist/**",
      "ds-bundle/**",
      "src-tauri/target/**",
      "**/*.css",
    ],
  },

  // Base JS
  js.configs.recommended,

  // TypeScript
  ...tseslint.configs.recommended,

  // React
  {
    files: ["**/*.{jsx,tsx}"],
    plugins: {
      react,
      "react-hooks": reactHooks,
    },
    rules: {
      ...react.configs.recommended.rules,
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "warn",
      "react/react-in-jsx-scope": "off",
    },
    settings: {
      react: {
        version: "19",
      },
    },
  },

  // Browser globals
  {
    files: ["**/*.{js,mjs,cjs,ts,mts,cts,jsx,tsx}"],
    languageOptions: {
      parserOptions: {
        ecmaFeatures: {
          jsx: true,
        },
      },
      globals: {
        ...globals.browser,
      },
    },
  },
  {
    files: ["**/*.cjs"],
    languageOptions: {
      globals: {
        ...globals.node,
        module: "writable",
        require: "readonly",
      },
    },
  },
  {
    files: ["src-tauri/crates/quadrant-napi/index.js"],
    languageOptions: {
      globals: {
        ...globals.node,
      },
    },
  },

  // CSS
  {
    files: ["**/*.css"],
    plugins: {
      css,
    },
    rules: {
      ...css.configs.recommended.rules,
    },
  },
  {
    rules: {
      "@typescript-eslint/no-explicit-any": "off",
      // Honor the underscore-prefix convention for intentionally-unused
      // bindings (e.g. API-compat constructor params, ignored catch vars).
      "@typescript-eslint/no-unused-vars": [
        "error",
        {
          argsIgnorePattern: "^_",
          varsIgnorePattern: "^_",
          caughtErrorsIgnorePattern: "^_",
        },
      ],
    },
  },
]);
