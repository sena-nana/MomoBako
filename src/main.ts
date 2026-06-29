import { createApp } from "vue";
import App from "./App.vue";
import { router } from "./router";
import {
  configureMomoBakoUiCore,
  installContextMenu,
  installGlobalScrollbarVisibility,
  useCornerStyle,
  useTheme,
  vContextMenu,
} from "./ui/core";
import "./styles/index.css";

configureMomoBakoUiCore();
useTheme();
useCornerStyle();
installContextMenu();
installGlobalScrollbarVisibility();

const app = createApp(App);
app.use(router);
app.directive("context-menu", vContextMenu);
app.mount("#root");
