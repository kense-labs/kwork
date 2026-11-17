// .tools/reference/deepseek-harness/packages/client/ui-theme/src/index.ts
import z2 from "@deepseek-ai/schemastery";

// .tools/reference/deepseek-harness/packages/client/ui-theme/src/theme-settings.ts
import z from "@deepseek-ai/schemastery";
var THEME_PREFERENCES = ["light", "dark", "system"];
var THEME_SETTINGS_NAMESPACE = "ui-theme";
var THEME_PREFERENCE_FIELD = "preference";
var FONT_SIZE_FIELD = "fontSize";
var DEFAULT_PREFERENCE = "system";
var FONT_SIZE_MIN = 10;
var FONT_SIZE_MAX = 22;
var DEFAULT_FONT_SIZE = 14;
var FONT_ROLES = ["text", "code", "terminal"];
var FONT_SIZE_SPECS = {
  text: { field: FONT_SIZE_FIELD, variable: "--dsh-content-font-size", min: FONT_SIZE_MIN, max: FONT_SIZE_MAX, default: DEFAULT_FONT_SIZE },
  code: { field: "codeFontSize", variable: "--dsh-code-font-size", min: 10, max: 16, default: 11 },
  terminal: { field: "terminalFontSize", variable: "--dsh-terminal-font-size", min: 10, max: 20, default: 13 }
};
var DEFAULT_FONT_SIZES = Object.freeze({
  text: FONT_SIZE_SPECS.text.default,
  code: FONT_SIZE_SPECS.code.default,
  terminal: FONT_SIZE_SPECS.terminal.default
});
var FONT_FAMILY_FIELDS = {
  text: "textFontFamily",
  code: "codeFontFamily",
  terminal: "terminalFontFamily"
};
function fontFamilyVariable(kind) {
  return `--dsh-font-family-${kind}`;
}
function fontSizeSchema(spec) {
  return z.number().step(1).min(spec.min).max(spec.max).default(spec.default);
}
var ThemeSettingsSchema = z.object({
  [THEME_PREFERENCE_FIELD]: z.union([...THEME_PREFERENCES]).default(DEFAULT_PREFERENCE),
  [FONT_SIZE_FIELD]: fontSizeSchema(FONT_SIZE_SPECS.text),
  [FONT_SIZE_SPECS.code.field]: fontSizeSchema(FONT_SIZE_SPECS.code),
  [FONT_SIZE_SPECS.terminal.field]: fontSizeSchema(FONT_SIZE_SPECS.terminal),
  [FONT_FAMILY_FIELDS.text]: z.string().default(""),
  [FONT_FAMILY_FIELDS.code]: z.string().default(""),
  [FONT_FAMILY_FIELDS.terminal]: z.string().default("")
});
var GENERIC_FAMILIES = /* @__PURE__ */ new Set([
  "serif",
  "sans-serif",
  "monospace",
  "cursive",
  "fantasy",
  "system-ui",
  "ui-serif",
  "ui-sans-serif",
  "ui-monospace",
  "ui-rounded",
  "emoji",
  "math",
  "fangsong",
  "-apple-system",
  "blinkmacsystemfont"
]);
function normalizeFontFamily(value) {
  return value.split(",").map((name) => name.replace(/["'\\<>\u0000-\u001f\u007f]/gu, "").replace(/\s+/gu, " ").trim()).filter((name) => name !== "").map((name) => GENERIC_FAMILIES.has(name.toLowerCase()) ? name : `"${name}"`).join(", ");
}

// .tools/reference/deepseek-harness/packages/client/ui-theme/src/boot-theme.ts
var LIGHT_BACKGROUND = "#fff";
var DARK_BACKGROUND = "#151517";
function bootThemeStyle(preference) {
  const light = `:root{color-scheme:light}body{background-color:${LIGHT_BACKGROUND};--dsh-boot-bg:${LIGHT_BACKGROUND}}`;
  const dark = `:root{color-scheme:dark}body{background-color:${DARK_BACKGROUND};--dsh-boot-bg:${DARK_BACKGROUND}}`;
  if (preference === "light") return light;
  if (preference === "dark") return dark;
  return `${light}@media(prefers-color-scheme:dark){${dark}}`;
}
function bootThemeBodyScript(preference, fontSizes, fontFamilies) {
  const sizes = FONT_ROLES.map((role) => `
  document.body.style.setProperty(${JSON.stringify(FONT_SIZE_SPECS[role].variable)}, ${JSON.stringify(`${fontSizes[role]}px`)})`).join("");
  const families = FONT_ROLES.flatMap((kind) => {
    const list = normalizeFontFamily(fontFamilies[kind]);
    return list === "" ? [] : [`
  document.body.style.setProperty(${JSON.stringify(fontFamilyVariable(kind))}, ${JSON.stringify(list)})`];
  }).join("");
  return `(() => {
  const preference = ${JSON.stringify(preference)}
  const systemDark = preference === 'system'
    && typeof matchMedia !== 'undefined'
    && matchMedia('(prefers-color-scheme: dark)').matches
  const dark = preference === 'dark' || systemDark
  document.documentElement.dataset.dsThemeSource = preference
  document.body.toggleAttribute('data-ds-dark-theme', dark)${sizes}${families}
})()`;
}
function bootThemeInjections(preference = DEFAULT_PREFERENCE, fontSizes = DEFAULT_FONT_SIZES, fontFamilies = { text: "", code: "", terminal: "" }) {
  return [
    { kind: "style", text: bootThemeStyle(preference) },
    { kind: "script", placement: "body", text: bootThemeBodyScript(preference, fontSizes, fontFamilies) }
  ];
}

// .tools/reference/deepseek-harness/packages/client/ui-theme/src/index.ts
var Config = z2.object({
  preference: z2.union([...THEME_PREFERENCES]).default(DEFAULT_PREFERENCE).volatile(),
  fontSize: fontSizeSchema(FONT_SIZE_SPECS.text).volatile(),
  codeFontSize: fontSizeSchema(FONT_SIZE_SPECS.code).volatile(),
  terminalFontSize: fontSizeSchema(FONT_SIZE_SPECS.terminal).volatile(),
  textFontFamily: z2.string().default("").volatile(),
  codeFontFamily: z2.string().default("").volatile(),
  terminalFontFamily: z2.string().default("").volatile()
});
function apply(ctx, config) {
  ctx.inject(["settings"], (child) => {
    child.effect(() => child.settings.configure({ auto: false }, ctx.fiber));
  });
  ctx.on("webserver/index-inject", (table) => {
    table.push(...bootThemeInjections(config.preference.get(), {
      text: config.fontSize.get(),
      code: config.codeFontSize.get(),
      terminal: config.terminalFontSize.get()
    }, {
      text: config.textFontFamily.get(),
      code: config.codeFontFamily.get(),
      terminal: config.terminalFontFamily.get()
    }));
  }, { prepend: true });
}
export {
  Config,
  DEFAULT_FONT_SIZE,
  DEFAULT_PREFERENCE,
  FONT_SIZE_FIELD,
  FONT_SIZE_MAX,
  FONT_SIZE_MIN,
  THEME_PREFERENCES,
  THEME_PREFERENCE_FIELD,
  THEME_SETTINGS_NAMESPACE,
  apply
};
