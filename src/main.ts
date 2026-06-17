import { createApp } from "vue";
import App from "./App.vue";
import { router } from "./router";
import "./composables/useTheme";
import "./composables/useCornerStyle";
import { installContextMenu } from "./composables/useContextMenu";
import { installGlobalScrollbarVisibility } from "./composables/useGlobalScrollbarVisibility";
import { vContextMenu } from "./directives/contextMenu";
import "./styles/index.css";

installContextMenu();
installGlobalScrollbarVisibility();

const app = createApp(App);
app.use(router);
app.directive("context-menu", vContextMenu);
app.mount("#root");
