import { createApp } from "vue";
import App from "./App.vue";
import { router } from "./router";
import {
  installContextMenu,
  installGlobalScrollbarVisibility,
  useCornerStyle,
  useTheme,
  vContextMenu,
} from "./ui";
import "./styles/index.css";

useTheme();
useCornerStyle();
installContextMenu();
installGlobalScrollbarVisibility();

const app = createApp(App);
app.use(router);
app.directive("context-menu", vContextMenu);
app.mount("#root");
