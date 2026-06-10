import { defineConfig } from "vitepress";

const repository = process.env.GITHUB_REPOSITORY?.split("/")[1];
const isProjectPages = repository && !repository.endsWith(".github.io");
const base = process.env.GITHUB_ACTIONS && isProjectPages ? `/${repository}/` : "/";

export default defineConfig({
  title: "MomoBako",
  description: "Tauri 2 + Vue 3 desktop resource library workspace.",
  base,
  themeConfig: {
    nav: [
      { text: "架构", link: "/architecture" },
      { text: "API", link: "/api-design" },
      { text: "样式", link: "/design/style-standard" },
    ],
    sidebar: [
      {
        text: "MomoBako",
        items: [
          { text: "概览", link: "/" },
          { text: "架构", link: "/architecture" },
          { text: "API 设计", link: "/api-design" },
          { text: "样式标准", link: "/design/style-standard" },
        ],
      },
    ],
    socialLinks: [],
  },
});
