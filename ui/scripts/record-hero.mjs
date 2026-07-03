// Records the M4 hero GIF (SPEC M4 / RESEARCH §5): drives the @hero Playwright
// spec on the headed WebGPU project against the fixture-replaying daemon, then
// converts the newest webm to a palette-optimized 10 s GIF (plus an mp4
// sibling) in docs/.
//
//   cd ui && npm run hero
//
// Requires ffmpeg on PATH (Windows: `winget install Gyan.FFmpeg`).
// D-009: the in-page fixture badge is in every frame; no post-editing needed.

import { execSync, spawnSync } from "node:child_process";
import { mkdirSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const GIF_SECONDS = 10;
const SKIP_SECONDS = 6; // skip page load; start during the household overview
const WIDTH = 880;
const FPS = 12;

try {
  execSync("ffmpeg -version", { stdio: "ignore" });
} catch {
  console.error("ffmpeg not found on PATH. Windows: winget install Gyan.FFmpeg");
  process.exit(1);
}

console.log("Recording hero footage (headed WebGPU chromium)…");
const run = spawnSync(
  "npx",
  ["playwright", "test", "e2e/hero.spec.ts", "--project=chromium-webgpu", "--grep", "@hero"],
  { stdio: "inherit", shell: true },
);
if (run.status !== 0) {
  console.error("hero spec failed");
  process.exit(run.status ?? 1);
}

const videoDir = "e2e-results/video";
const newest = readdirSync(videoDir)
  .filter((f) => f.endsWith(".webm"))
  .map((f) => join(videoDir, f))
  .sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs)[0];
if (!newest) {
  console.error(`no webm found in ${videoDir}`);
  process.exit(1);
}
console.log(`Converting ${newest} → docs/hero.gif`);

mkdirSync("../docs", { recursive: true });
const filters = `fps=${FPS},scale=${WIDTH}:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4`;
execSync(
  `ffmpeg -y -ss ${SKIP_SECONDS} -t ${GIF_SECONDS} -i "${newest}" -vf "${filters}" ../docs/hero.gif`,
  { stdio: "inherit" },
);
execSync(
  `ffmpeg -y -ss ${SKIP_SECONDS} -t ${GIF_SECONDS} -i "${newest}" -vf "scale=${WIDTH}:-2" -c:v libx264 -pix_fmt yuv420p -crf 23 -an ../docs/hero.mp4`,
  { stdio: "inherit" },
);

const gifBytes = statSync("../docs/hero.gif").size;
console.log(`docs/hero.gif: ${(gifBytes / 1e6).toFixed(2)} MB (target < 10 MB)`);
if (gifBytes > 10_000_000) {
  console.warn("GIF exceeds 10 MB — re-encode with WIDTH=800 / FPS=12 in this script.");
}
