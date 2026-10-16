# Q-plt-trunk-version-pin

- **Agent session:** pages-deploy subagent (no session id known to me)
- **Task:** GitHub Pages deployment for the web build (`.github/workflows/pages.yml`, `packaging/web/README-pages.md`)
- **Owned files:** `.github/workflows/pages.yml`, `packaging/web/README-pages.md`
- **Status:** ASSUMED-AND-CONTINUING

## What is ambiguous

No Trunk version is recorded anywhere in the repo: `apps/cadcraft-web/Trunk.toml` has no
version field by design, and `packaging/web/package.sh` (line 6) just says `cargo install
trunk --locked` / `brew install trunk`. The new `pages.yml` therefore installs Trunk via
`jetli/trunk-action@v0.5.0` with `version: latest`, and neither `package.sh` nor `Trunk.toml`
was in my owned files, so I could not record a pin at the source.

## The options I can see

1. **Owner picks an exact Trunk version** (e.g. the one that built the first green Pages
   deploy) and it is set in `pages.yml` (`version:`) and noted in `packaging/web/package.sh`.
   Consequence: reproducible builds; a Trunk upgrade becomes a deliberate PR that either stays
   green or gets reverted.
2. **Keep floating at `latest`.** Consequence: zero maintenance until a Trunk release changes
   output or flags, at which point the Pages build goes red (loud, not silent) and someone
   pins then.

## What I am doing meanwhile

Option 2 inflight (`version: latest` in `.github/workflows/pages.yml`), because inventing a
specific version number I cannot verify would be worse than floating: a wrong pin fails every
build, while `latest` builds today and fails loudly only if Trunk itself breaks us. This is the
least-damaging option if wrong — a red build step, not a silently wrong site.

## What would change if the answer is the other option

If option 1: one line in `.github/workflows/pages.yml` (`version: latest` → the pinned
version) plus a matching note in `packaging/web/package.sh` so local `package.sh` builds use
the same Trunk; `packaging/web/README-pages.md` “Known gaps” entry shrinks to a pointer.
