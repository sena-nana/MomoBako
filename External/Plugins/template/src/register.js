export function register(ctx) {
  ctx.registerPreview({
    supportedExtensions: ["txt"],
    component: {
      name: "TemplatePluginPreview",
      template: `
        <section class="plugin-template-preview">
          <header>Template Plugin</header>
          <p>当前文件: {{ entry?.name ?? "Unknown" }}</p>
          <p>插件: {{ pluginName }}</p>
        </section>
      `,
      props: {
        entry: {
          type: Object,
          default: null,
        },
      },
      data() {
        return {
          pluginName: ctx.manifest.name,
        };
      },
    },
  });
}
