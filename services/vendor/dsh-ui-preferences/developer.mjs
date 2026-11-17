// .tools/reference/deepseek-harness/packages/client/ui-settings/src/index.ts
import z2 from "@deepseek-ai/schemastery";

// .tools/reference/deepseek-harness/packages/client/ui-settings/src/developer-tools-settings.ts
import z from "@deepseek-ai/schemastery";
var DeveloperToolsSettingsFields = {
  enabled: z.boolean().default(true)
};
var DeveloperToolsSettingsSchema = z.object(DeveloperToolsSettingsFields);

// .tools/reference/deepseek-harness/packages/client/ui-settings/src/index.ts
var Config = z2.object({
  enabled: DeveloperToolsSettingsFields["enabled"].volatile()
});
function apply(ctx) {
  ctx.inject(["settings"], (child) => {
    child.effect(() => child.settings.configure({ auto: false }, ctx.fiber));
  });
}
export {
  Config,
  apply
};
