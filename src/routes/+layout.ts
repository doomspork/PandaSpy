// Tauri loads the frontend from the filesystem: there is no server, so nothing
// can be rendered on one. Prerender the shell, run everything else in the
// webview.
export const prerender = true;
export const ssr = false;
