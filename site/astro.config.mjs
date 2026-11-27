// @ts-check
import { defineConfig } from "astro/config";

// The canonical site is razdor.indicozy.dev; the GitHub Pages copy is built with
// RAZDOR_SITE_BASE=/razdor/ (every internal link goes through import.meta.env.BASE_URL).
export default defineConfig({
  site: "https://razdor.indicozy.dev",
  base: process.env.RAZDOR_SITE_BASE ?? "/",
  trailingSlash: "always",
  build: { format: "directory" },
});
