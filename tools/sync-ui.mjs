// Copies the player code shared between the extension and the companion app.
// Usage: node tools/sync-ui.mjs
import { copyFileSync, mkdirSync } from 'node:fs';

const shared = ['util.js', 'styles.js', 'player-ui.js'];
const out = new URL('../companion/ui/shared/', import.meta.url);
mkdirSync(out, { recursive: true });
for (const f of shared) copyFileSync(new URL(`../src/content/${f}`, import.meta.url), new URL(f, out));
console.log(`synced ${shared.length} files to companion/ui/shared/`);
