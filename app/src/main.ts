import { createApp } from "vue";
import { createPinia } from "pinia";
import CadLayout from "./components/CadLayout.vue";
import "./style.css";

// Single-window layout: CadLayout is the app shell (toolbar + feature tree
// + properties panel + status bar). The native wgpu surface renders the 3D
// scene in the window's center region; its rectangle is computed from the
// same metrics as the CSS grid (see --toolbar-height / --sidebar-*-width in
// style.css and LAYOUT in src-tauri/src/wgpu_viewport.rs).
const app = createApp(CadLayout);
app.use(createPinia());
app.mount("#app");
