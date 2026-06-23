# Recollect web shell (D-18)

A browser client rendering the canon engine. The shell (`index.html`) renders
the EXACT JSON the WASM core emits — `recollect_core::view::view_for` →
`PlayerView` — so client and server agree on the rules by construction (one
crate). Combat previews are exact, not approximated.

## Build (wasm)
```
cd app/crates/recollect-web
trunk build --release          # or: wasm-pack build --target web --out-dir shell/pkg
# then serve shell/ statically
```
The shell's `render(v)` takes a parsed `PlayerView`. In the wired build:
```js
import init, { LocalGame } from './pkg/recollect_web.js';
await init();
const game = LocalGame.new_quick(seed, yourStyle, oppStyle);
render(JSON.parse(game.view_json()));
```
Standalone (no wasm), `index.html` renders a shape-identical sample so the
visual design can be reviewed without a toolchain. The renderer is identical.

## Design language
Aged paper; two inks (teal = Narrator A, sienna = B) that bleed into impressions;
a 12-pip clock that darkens past round 8 (the Dusk); lamplit Held tiles; the
Echo mark (◌). Identity, not chrome — distinct from the in-app widget look.

## 2v2 (D-16)
`view_for_slot` → `TeamView` carries `board_w` (6) and four-seat counts; the
same renderer scales the grid to 6×6. The four-socket lobby is D-16's
red-contracted remainder.
