import eslint from "@eslint/js";
import hooks from "eslint-plugin-react-hooks";
import tseslint from "typescript-eslint";

export default tseslint.config(
  eslint.configs.recommended,
  ...tseslint.configs.recommended,
  {
    plugins: { "react-hooks": hooks },
    rules: {
      ...hooks.configs.recommended.rules,
      // Existing code deliberately uses setState inside effects for fetch-on-
      // activate patterns; the new v6 rule would flag all of them.
      "react-hooks/set-state-in-effect": "off",
    },
  },
  {
    ignores: ["dist/", "node_modules/", "src/api/schema.d.ts"],
  },
);
