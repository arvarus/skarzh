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

## Session

skarzh works on one session at a time. A session is the user's setup (input folders, output folder, path template) plus everything the application derives from it (scan results, thumbnail cache, perceptual hashes, later review decisions). At each launch the user continues the previous session or starts a new one: a new session deletes all the application data of the previous one. The input and output folders belong to the user: the session logic never modifies or deletes anything in them (it only creates the output folder when it is missing).

The code lives in `src-tauri/src/session/` (plain Rust, unit-tested on temporary directories); the commands in `src-tauri/src/lib.rs` are thin wrappers around it.

### Storage

Everything is under the application local data directory (`%LOCALAPPDATA%\com.arvarus.skarzh`):

```text
session/
  session.json   schemaVersion, config, createdAt, updatedAt (RFC 3339 UTC); written atomically
  thumbnails/    reserved for the thumbnail cache
  cache/         reserved for hashes, scan data, review decisions...
```

`session.json` is written to a temporary file in the same directory, flushed, then renamed over the target, so a crash never leaves a partial file. A file that is corrupted, or written by a newer schema version, is reported as `unreadable` and never overwritten: the only way out is a reset. Resuming changes nothing on disk. All data derived from a session must live under `session/`, so that a reset wipes it.

A reset deletes `session/` recursively. It refuses (`reset_unsafe`, nothing touched) unless the directory is exactly `<app data>/session`, is not a filesystem root, and neither contains nor lies inside any configured input or output folder. Links (symlinks, junctions) are never followed: a link found inside is removed itself, its target is left alone. Backend jobs added later (scan, thumbnails, hashing) must be cancelled before `session_reset`.

### Commands

On failure every command returns an error `{ "code": string, "message": string }`; switch on `code`.

| Command                                | Result                                                                                            |
| -------------------------------------- | ------------------------------------------------------------------------------------------------- |
| `session_status()`                     | `{ state: "none" }`, `{ state: "existing", summary }` or `{ state: "unreadable", reason }`        |
| `session_create({ config })`           | the created `Session`; `session_already_exists` when anything (even unreadable) is already there  |
| `session_get()` / `session_resume()`   | the saved `Session` (`no_session`, `session_corrupted`, `session_schema_too_new`...)              |
| `session_reset()`                      | nothing; idempotent (no session is not an error)                                                  |
| `validate_path_template({ template })` | the template normalised to `/` separators                                                         |
| `preview_path_template({ template })`  | the template rendered with a fixed sample: 2024-07-14, `Canon EOS R6`, `Holidays`, `IMG_0042.jpg` |

JSON shapes (camelCase):

```ts
type SessionConfig = {
  inputFolders: string[];
  outputFolder: string;
  pathTemplate: string;
};
type Session = {
  schemaVersion: number;
  config: SessionConfig;
  createdAt: string; // RFC 3339 UTC
  updatedAt: string;
};
type SessionSummary = {
  inputFolders: string[];
  outputFolder: string;
  createdAt: string;
  updatedAt: string;
};
type AppError = { code: string; message: string };
type SessionStatus =
  | { state: "none" }
  | { state: "existing"; summary: SessionSummary }
  | { state: "unreadable"; reason: AppError };
```

`session_create` validates the configuration, creates the output folder if needed and stores the path template normalised. Error codes:

| Code                                                                                | Meaning                                                                |
| ----------------------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `no_input_folders`                                                                  | no input folder                                                        |
| `path_not_absolute`                                                                 | an input or the output path is relative                                |
| `input_not_found`, `input_not_a_directory`                                          | an input does not exist, or is a file                                  |
| `duplicate_input`                                                                   | the same folder twice (links resolved, case-insensitive)               |
| `nested_inputs`                                                                     | an input is inside another input                                       |
| `output_inside_input`, `input_inside_output`                                        | the output and an input overlap (an identical folder counts as inside) |
| `output_not_a_directory`, `output_create_failed`                                    | the output is a file, or cannot be created                             |
| `session_dir_conflict`                                                              | a folder is, contains or lies inside the application session directory |
| `session_already_exists`                                                            | `session_create` while a session exists: reset first                   |
| `no_session`, `session_file_missing`, `session_corrupted`, `session_schema_too_new` | reading problems (also reported by `session_status`)                   |
| `reset_unsafe`                                                                      | the reset safety checks refused to delete                              |
| `io`                                                                                | other i/o error                                                        |
| `template_*`                                                                        | invalid path template, see below                                       |

### Path template

A template is a relative path built from literal text and tokens, for example `{year}/{month}/{camera}/{filename}`. Tokens: `{year}` (4 digits), `{month}` and `{day}` (2 digits), `{camera}` (`unknown-camera` when empty), `{original_folder}` and `{filename}`. Values are sanitised when rendered: characters invalid on Windows and control characters become `_`, trailing dots and spaces are trimmed, and a segment that would be empty or a reserved Windows name is made safe (`_` or a `_` prefix). Each rule has its own error code:

| Code                                                                                     | Rule                                                                                  |
| ---------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| `template_empty`                                                                         | the template is not empty                                                             |
| `template_unknown_token`, `template_unbalanced_braces`, `template_nested_braces`         | only the six tokens, with balanced and non-nested braces                              |
| `template_absolute_path`, `template_drive_letter`, `template_unc_path`                   | relative path: no leading `/` or `\`, no `C:`, no `\\server`                          |
| `template_dot_segment`, `template_empty_segment`                                         | no `.`, `..` or empty segment (`a//b`, trailing separator); `/` and `\` both accepted |
| `template_invalid_character`                                                             | no `<>:"\|?*` or control character in literal text                                    |
| `template_reserved_name`                                                                 | no literal segment named `CON`, `PRN`, `AUX`, `NUL`, `COM1`-`COM9`, `LPT1`-`LPT9`     |
| `template_invalid_segment_end`                                                           | a literal segment does not end with a dot or a space                                  |
| `template_missing_filename`, `template_duplicate_filename`, `template_filename_not_last` | `{filename}` appears exactly once, in the last segment (`{year}_{filename}` is fine)  |
| `template_too_long`                                                                      | rendered with the longest plausible values, the path stays within 240 characters      |

Applying a template to real files is not implemented yet.

## Continuous integration and releases

GitHub Actions workflows (`.github/workflows/`):

- `ci.yml`: on every pull request and push to `main`, a single job on `windows-latest` that installs Node 24 and the stable Rust toolchain (with clippy and rustfmt) and runs `npm ci` then `npm run verify`.
- `pr-title.yml` and `commits.yml`: pull requests are squash-merged, so the PR title must follow [Conventional Commits](https://www.conventionalcommits.org) (`feat(scope): ...`, `fix: ...`, `chore: ...`), and so must every commit of the PR. Check a message locally with `echo "feat: add a thing" | npx commitlint`.
- `release-please.yml`: [release-please](https://github.com/googleapis/release-please) keeps a release pull request open on `main`, built from the Conventional Commits merged since the last release. It bumps the version in `package.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` and `src-tauri/tauri.conf.json` and writes `CHANGELOG.md`. Merging that pull request creates the `vX.Y.Z` tag and the GitHub release. No installer is built or attached yet.

The release workflow needs a `RELEASE_PLEASE_TOKEN` repository secret (a fine-grained token on this repository with read and write access to Contents and Pull requests), so that the checks run on the release pull request.

## Project layout

- `src/`: Svelte front end (`routes/` for pages, `lib/` for shared code and its tests).
- `src-tauri/`: Rust backend (`src/lib.rs` holds the commands, `src/session/` the session logic, `tauri.conf.json` the app configuration, `capabilities/` the permissions granted to the front end).

## License

[MIT](LICENSE)
