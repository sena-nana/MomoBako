declare module "vue3-markdown-it" {
  import type { DefineComponent } from "vue";

  const Markdown: DefineComponent<{
    source: string;
  }>;

  export default Markdown;
}
