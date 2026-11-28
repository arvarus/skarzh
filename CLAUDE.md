# skarzh

Desktop photo tool. Tauri 2 (Rust backend, system webview = WebView2) with a Svelte 5 + TypeScript front end (SvelteKit with `adapter-static`, SPA mode, `ssr = false`). MIT license. Windows only for now (macOS maybe later): do not add Linux or macOS specifics.

## Commands

Rust tools live in the rustup/cargo bin directory; make sure `cargo` is on the PATH.

- `npm install`: install the front-end dependencies.
- `npm run tauri dev`: run the app (Vite on port 1420 + Rust build, hot reload).
- `npm run tauri build -- --no-bundle`: release build of the executable, no installer.
- `npm run verify`: all checks (front + Rust). Must pass before every commit.
  - `npm run verify:front`: `lint` (ESLint), `format:check` (Prettier), `check` (svelte-check), `test` (Vitest).
  - `npm run verify:rust`: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` on `src-tauri/Cargo.toml`.
- `npm run format`: rewrite files with Prettier (run `cargo fmt --manifest-path src-tauri/Cargo.toml` for Rust).

## Architecture

- `src/`: front end. `routes/` holds the SvelteKit pages, `lib/` the shared TypeScript. Backend calls go through typed wrappers around `invoke` (see `src/lib/app-info.ts`), never raw `invoke` calls in components.
- `src-tauri/`: backend. `src/lib.rs` defines the Tauri commands and `run()`; `src/main.rs` only calls `skarzh_lib::run()`. Keep logic in plain functions (unit-testable without a Tauri runtime) and keep `#[tauri::command]` functions thin.
- `src-tauri/src/session/`: the session (one at a time: the user's setup plus everything derived from it). Submodules: `config` (data model, camelCase JSON), `error` (`SessionError`, serialised as `{ code, message }` with stable codes), `paths` (`SessionPaths`: `<app local data>/session/` with `session.json`, `thumbnails/`, `cache/`, plus path comparison helpers), `fsutil` (atomic write, link-safe removal), `validation` (rules checked on create), `store` (`SessionStore`: status, create, get, resume, reset), `template` (path template parser, validator, renderer). The store takes its root directory as a parameter (temp dirs in tests); `lib.rs` injects the real app local data dir in a `Mutex`-guarded `SessionState` built in `setup`.
  - All data derived from a session (scan results, thumbnails, hashes, review decisions) must live under `SessionPaths::dir()`, so that `session_reset` wipes it.
  - Session logic never modifies or deletes anything in the input and output folders.
  - `session.json` is only written through `fsutil::write_atomic`, and an unreadable one (corrupted, newer schema) is never overwritten: it is reset.
  - Background jobs (scan, thumbnails, hashing) must be cancelled before `session_reset`.
- `src-tauri/capabilities/`: permissions granted to the webview. Grant only what is used.
- Tests: Vitest next to the code (`*.test.ts`, mock `@tauri-apps/api/core`); Rust unit tests in `#[cfg(test)]` modules.

## Stack decisions

- npm is the package manager; `package-lock.json` and `src-tauri/Cargo.lock` are committed.
- Svelte + SvelteKit static SPA as provided by `create-tauri-app` (`svelte-ts` template); do not switch frameworks.
- Bundle config is the scaffold default; no installer, signing or CI licence checks yet, and releases only tag and create a GitHub release without attaching an installer (planned in later tickets, as are folder scan, EXIF, thumbnails, perceptual hashes and the virtualised grid).
- Package and crate are `private`/`publish = false`; license `MIT`; identifier `com.arvarus.skarzh`.

## CI and releases

- `.github/workflows/ci.yml`: one job on `windows-latest` (Node 24, stable Rust with clippy and rustfmt, `Swatinem/rust-cache` on `src-tauri`) running `npm ci` and `npm run verify`, the same command as locally. Triggered by pull requests and pushes to `main`; superseded runs are cancelled.
- `.github/workflows/pr-title.yml` (PR title) and `commits.yml` (every commit of the PR) run commitlint with `commitlint.config.js` (`@commitlint/config-conventional`). PRs are squash-merged, so the title is what lands on `main` and drives the changelog and the version bump.
- `.github/workflows/release-please.yml` + `release-please-config.json` + `.release-please-manifest.json`: release-please (release type `node`, `bump-minor-pre-major`) opens and updates a release PR on `main`; merging it tags `vX.Y.Z` and creates the GitHub release. Its `extra-files` keep the version in `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` and `src-tauri/tauri.conf.json` in sync with `package.json`: do not bump versions by hand.
- The `Cargo.lock` entry is matched with the jsonpath `$.package[?(@.name.value=='skarzh')].version`: release-please's TOML parser wraps every value in a `{start, end, value}` object, hence `.name.value`.
- `.gitattributes` (`* text=auto eol=lf`) forces LF in the repository and in every checkout, whatever `core.autocrlf` is: needed by Prettier on the Windows runner (which checks out with `autocrlf=true`) and by release-please's TOML parser (CRLF in `src-tauri/Cargo.toml` makes it fail and silently skip the file). Keep binary files (icons) listed there as `binary`.
- `CHANGELOG.md` is generated by release-please (ignored by Prettier): never edit it by hand.
- Required repository setting: the `RELEASE_PLEASE_TOKEN` secret (fine-grained token, Contents and Pull requests read/write), so that CI runs on the release PR. Actions are pinned to a commit SHA with the version in a comment.

## Conventions

- English everywhere: code, comments, docs, commit messages.
- Conventional Commits (`feat(scope): ...`, `fix:`, `chore:`, `docs:`, ...), one subject per commit; CI checks the commits and the PR title.
- One ticket = one branch = one pull request.
- `npm run verify` must pass before committing.
- Update README and this file when commands, layout or decisions change. Do not edit generated files (`src-tauri/gen/`, lockfiles by hand).
