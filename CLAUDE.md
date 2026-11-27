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
- `src-tauri/capabilities/`: permissions granted to the webview. Grant only what is used.
- Tests: Vitest next to the code (`*.test.ts`, mock `@tauri-apps/api/core`); Rust unit tests in `#[cfg(test)]` modules.

## Stack decisions

- npm is the package manager; `package-lock.json` and `src-tauri/Cargo.lock` are committed.
- Svelte + SvelteKit static SPA as provided by `create-tauri-app` (`svelte-ts` template); do not switch frameworks.
- Bundle config is the scaffold default; no installer, signing or CI licence checks yet (planned in later tickets, as are folder scan, EXIF, thumbnails, perceptual hashes and the virtualised grid).
- Package and crate are `private`/`publish = false`; license `MIT`; identifier `com.arvarus.skarzh`.

## Conventions

- English everywhere: code, comments, docs, commit messages.
- Conventional Commits (`feat(scope): ...`, `fix:`, `chore:`, `docs:`, ...), one subject per commit.
- One ticket = one branch = one pull request.
- `npm run verify` must pass before committing.
- Update README and this file when commands, layout or decisions change. Do not edit generated files (`src-tauri/gen/`, lockfiles by hand).
