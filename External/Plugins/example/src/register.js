export function register(ctx) {
  ctx.registerPreview({
    supportedExtensions: ["txt", "md"],
    component: {
      name: "ExampleTextPreview",
      template: `
        <article class="example-text-preview">
          <p class="example-text-preview__eyebrow">Example Plugin</p>
          <h2>{{ entry?.name ?? "Unknown file" }}</h2>
          <p>这个视图来自独立的 .momoplug 前端 bundle。</p>
          <p>插件 ID: {{ pluginId }}</p>
        </article>
      `,
      props: {
        entry: {
          type: Object,
          default: null,
        },
      },
      data() {
        return {
          pluginId: ctx.manifest.pluginId,
        };
      },
    },
  });
}
