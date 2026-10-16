# CADCraft on GitHub Pages

The `pages` workflow (`.github/workflows/pages.yml`) builds the browser application and publishes
it to GitHub Pages, so the owner can open CADCraft in a browser on any device. There is no
server-side code: the published artifact is a static site.

## What the workflow does

On every push to the `main` branch (and on manual `workflow_dispatch` runs), two jobs run:

1. **`build`** (Ubuntu runner):
   - checks out the repo, installs the stable Rust toolchain with the
     `wasm32-unknown-unknown` target, restores the cargo/target cache
     (`Swatinem/rust-cache`, which covers the wasm target dir too);
   - installs Trunk, then runs `trunk build --release` in `apps/cadcraft-web`
     (config: `apps/cadcraft-web/Trunk.toml` — output goes to `dist/web/`,
     `public_url = "./"`, hashed asset names);
   - verifies `dist/web/index.html` exists and contains no root-absolute
     `src="/…"` / `href="/…"` URLs (the same guard `packaging/web/package.sh`
     applies — absolute URLs would break under the Pages sub-path);
   - uploads `dist/web/` with `actions/upload-pages-artifact`.
2. **`deploy`**: publishes that artifact with `actions/deploy-pages` into the
   `github-pages` environment. A newer push cancels an older queued or
   in-progress deploy (`concurrency: group: pages, cancel-in-progress: true`),
   so the live site always converges on the newest commit.

Action pins follow the house style of `.github/workflows/agent-check.yml`
(major-version tags: `actions/checkout@v4`, `dtolnay/rust-toolchain@stable`,
`actions/upload-pages-artifact@v3`, `actions/deploy-pages@v4`,
`jetli/trunk-action@v0.5.0`, `Swatinem/rust-cache@v2`). Trunk *itself* floats
at `latest` — no Trunk version is recorded anywhere in this repo — see
“Known gaps” below.

## One-time repository setting (owner)

Without this, the workflow builds the artifact and the deploy succeeds, but GitHub never serves it:

**Settings → Pages → Build and deployment → Source: select “GitHub Actions”**
(instead of “Deploy from a branch”). Nothing else needs changing; the `github-pages`
environment is created automatically on the first deploy.

## The URL

For the repository `test-personal` under the `emilio-n8n` account, the site appears at:

**`https://emilio-n8n.github.io/test-personal/`**

GitHub Pages serves project sites under a **sub-path** (`/test-personal/`), not at the domain
root. That works here because the build uses only relative URLs
(`public_url = "./"` in `apps/cadcraft-web/Trunk.toml` — see `packaging/web/README.md`
“Any path works”): the page loads its JS/wasm glue relative to wherever `index.html` was
served from. Do not introduce a root-absolute URL into `apps/cadcraft-web/index.html`; the
workflow's guard step fails the build if one appears.

## How to check the deployment

1. **Actions tab** → the `pages` run for the pushed commit is green; the `deploy` job's
   `github-pages` environment links the live URL.
2. **Open the URL** in a desktop or mobile browser (hard-refresh once after the first deploy;
   `index.html` is the only file that must revalidate — hashed `.js`/`.wasm` assets cache forever).
3. **If the app does not load**, open DevTools → Network and check:
   - `index.html` returns 200;
   - the `*_bg.wasm` file is served as `application/wasm` (Pages does this by default;
     under any other MIME type browsers refuse to stream-compile it and the app loads
     slowly or not at all — see `packaging/web/README.md` “Required server settings”);
   - no request goes to the domain root (a 404 there means a root-absolute URL slipped in).
4. Renderer fallback flags work on the URL too: append `?webgl` to force the WebGL2 backend
   or `?cpu` for the CPU canvas path when a WebGPU driver misbehaves.

## Expected download size

The compiled app (`*_bg.wasm`) is **about 13 MB**, or **about 5 MB over the wire** with
compression (GitHub Pages serves gzip). First load on a phone takes a few seconds on a normal
connection; repeat visits only re-fetch `index.html`.

## Plainly: what this build is — and is not

**The exported page is the 2D drafting application and nothing more. The 3D viewport is NOT
part of this build.** There is no interactive 3D viewport in the browser: the GPU render path
it would need (`REN-001`) is backlog work (`docs/plan/PLT.md`, `PLT-WASM-012`), and the repo
has no 3D view today (`docs/hardware-parity.md`: “no 3D”). What you get on the page is the
2D canvas, the command line, menus and dialogs — the ~150 000 lines of working 2D CAD,
visible at last. When the 3D viewport lands, this document must be updated to say so.

## Known gaps (not verified — no terminal here)

- **Green CI ≠ proven site.** `cargo xtask ci`'s `wasm` gate only runs
  `cargo check --target wasm32-unknown-unknown` per crate (`xtask/src/main.rs`, `cmd_wasm`).
  A type-check is not a Trunk build: `wasm-bindgen`, asset hashing, `index.html` rewriting
  and Trunk-version drift can all break the site while every other gate stays green.
  **This Pages workflow is currently the only thing that builds the real site** — keep it green.
- **Trunk version floats** (`version: latest` in the workflow). If a future Trunk release
  changes the build, the failure will be loud (install or build step goes red), and the fix
  is to pin the last good version. Open question with the owner:
  `docs/agents/questions/Q-plt-trunk-version-pin.md`.
- **Branch filter assumes `main`.** The workflow deploys on pushes to `main` (the default
  branch of the upstream this fork tracks). If the `test-personal` repository uses a different
  default-branch name, change `branches: [main]` in `pages.yml` to match — otherwise pushes
  will never deploy.
- **First deploy needs the one-time setting above.** Until Source is set to GitHub Actions,
  runs will build fine and the site will not exist.
