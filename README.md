# skarzh

Desktop photo tool built with [Tauri 2](https://tauri.app) (Rust backend, system webview) and [Svelte](https://svelte.dev) (SvelteKit in static SPA mode, TypeScript).

Target platform: Windows (macOS possibly later).

> Status: project skeleton. The window shows the application name and version, read from the Rust backend through a Tauri command. Features are added in later tickets.

## Prerequisites (Windows)

- [Rust](https://rustup.rs) via `rustup` (stable toolchain, MSVC target `x86_64-pc-windows-msvc`).
- [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) (Visual Studio 2022 Build Tools) with the "Desktop development with C++" workload, which includes the MSVC toolchain and the Windows SDK.
- [WebView2 runtime](https://developer.microsoft.com/microsoft-edge/webview2/) (already installed on Windows 11).
- [Node.js](https://nodejs.org) (LTS or newer) with npm.

See the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for details.

## Getting started

```sh
npm install
npm run tauri dev
```

The first run compiles the Rust dependencies, which takes a few minutes. Vite serves the front end on `http://localhost:1420` with hot reload; Rust changes trigger a rebuild.

## Build

```sh
npm run tauri build -- --no-bundle
```

This produces the executable at `src-tauri/target/release/skarzh.exe` without generating an installer. Installer packaging is not configured yet.

## Checks

| Command                | What it does                                                                      |
| ---------------------- | --------------------------------------------------------------------------------- |
| `npm run lint`         | ESLint (TypeScript and Svelte)                                                    |
| `npm run format:check` | Prettier check (`npm run format` rewrites the files)                              |
| `npm run check`        | `svelte-check` (type-checks TypeScript and Svelte)                                |
| `npm run test`         | Vitest unit tests of the front end                                                |
| `npm run verify:front` | The four commands above                                                           |
| `npm run verify:rust`  | `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` |
| `npm run verify`       | `verify:front` then `verify:rust`: run it before every commit                     |

## Continuous integration and releases

GitHub Actions workflows (`.github/workflows/`):

- `ci.yml`: on every pull request and push to `main`, a single job on `windows-latest` that installs Node 24 and the stable Rust toolchain (with clippy and rustfmt) and runs `npm ci` then `npm run verify`.
- `pr-title.yml` and `commits.yml`: pull requests are squash-merged, so the PR title must follow [Conventional Commits](https://www.conventionalcommits.org) (`feat(scope): ...`, `fix: ...`, `chore: ...`), and so must every commit of the PR. Check a message locally with `echo "feat: add a thing" | npx commitlint`.
- `release-please.yml`: [release-please](https://github.com/googleapis/release-please) keeps a release pull request open on `main`, built from the Conventional Commits merged since the last release. It bumps the version in `package.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` and `src-tauri/tauri.conf.json` and writes `CHANGELOG.md`. Merging that pull request creates the `vX.Y.Z` tag and the GitHub release. No installer is built or attached yet.

The release workflow needs a `RELEASE_PLEASE_TOKEN` repository secret (a fine-grained token on this repository with read and write access to Contents and Pull requests), so that the checks run on the release pull request.

## Project layout

- `src/`: Svelte front end (`routes/` for pages, `lib/` for shared code and its tests).
- `src-tauri/`: Rust backend (`src/lib.rs` holds the commands, `tauri.conf.json` the app configuration, `capabilities/` the permissions granted to the front end).

## License

[MIT](LICENSE)
