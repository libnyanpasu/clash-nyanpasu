# Contributing to Nyanpasu

Welcome to **Nyanpasu** development!  
To ensure the quality and stability of the project, please read this guide carefully. Even if you are new, you can follow these steps to set up the development environment, write code, and submit contributions.

---

## 1. Development Guidelines

Read the [development standards](docs/development/README.md) before making changes. They describe the shared architecture, unified RPC, testing, worktree, and commit requirements for human contributors and coding agents.

Before submitting code, please follow these rules:

### 1. Code Style Checks

| Language                | Tools                       |
| ----------------------- | --------------------------- |
| JavaScript / TypeScript | Oxlint, Prettier, Stylelint |
| Rust                    | Clippy, Rustfmt             |

> [!WARNING]
>
> - ⚠️ **Ensure there are no style errors before committing**
> - ❌ **Do not use `git commit -n` or skip checks**, CI will automatically enforce style validation

### 2. Submission Requirements

- Avoid submitting useless code, files, or folders
- For major refactors or new features, open an **Issue** first for discussion
- If unsure about implementation or have questions, communicate in **Issue** or **PR**

### 3. Communication & Collaboration

- Respect others' code and opinions
- Keep commit messages and PR descriptions clear
- All discussions should be on GitHub for transparency and traceability

---

## 2. Environment Requirements

To ensure the project runs correctly locally, the following dependencies are required.

### 1. Required Dependencies

| Tool    | Version | Link                                                        | Notes                                                                                                   |
| ------- | ------- | ----------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| Rust    | Nightly | [Official Install](https://www.rust-lang.org/tools/install) | Install via `rustup`; the root `rust-toolchain.toml` selects nightly automatically; use MSVC on Windows |
| Node.js | 24      | [Official Site](https://nodejs.org/)                        | Matches `engines.node` in `package.json`                                                                |
| pnpm    | 12      | [Official Documentation](https://pnpm.io/)                  | Pinned by `packageManager` in `package.json`; Corepack can install the exact version                    |
| Deno    | 2.x     | [Official Site](https://deno.com/)                          | Runs repository automation (`deno task ...`)                                                            |
| git     | Latest  | [Official Site](https://git-scm.com/)                       | Version control                                                                                         |

### 2. Build Dependencies

| Tool  | Link                                | Notes                                                      |
| ----- | ----------------------------------- | ---------------------------------------------------------- |
| cmake | [Official Site](https://cmake.org/) | Build dependency of `aws-lc-sys` (TLS backend `aws-lc-rs`) |

On Linux, also install the Tauri system libraries, for example on Debian/Ubuntu:

```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libxdo-dev libappindicator3-dev librsvg2-dev patchelf
```

See the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for other platforms.

### 3. Windows Special Requirements

- Always use the **MSVC toolchain** on Windows (install the Visual Studio C++ build tools)
- Creating directory symlinks for [worktrees](docs/development/workflow.md) requires Developer Mode

---

## 3. Pre-Development Setup

Before starting development, install Deno 2 for repository automation, initialize the environment and download required resources.

### 1. Install Frontend Dependencies

```bash
pnpm i
```

> [!TIP]
> This installs all frontend dependencies including UI components, toolchains, and testing tools.

### 2. Download Core & Resource Files

```
deno task prepare:check
```

If files are missing or you want to force update:

```
deno task prepare:check --force
```

> [!TIP]
>
> - This command downloads binaries like `sidecar` and `resource` to ensure the project runs properly
> - Configure terminal proxy if network issues occur

---

## 4. Start Development Environment

The project provides two types of development instances:

### 1. Dedicated Development Instance (Recommended)

```
pnpm dev:diff
```

> [!TIP]
> Suitable for daily development and debugging; changes do not affect the release version

### 2. Release-Like Development Instance

```
pnpm dev
```

> [!TIP]
> Behaves similarly to the official release; useful to test overall functionality

### 3. Build application

```shell
pnpm build
```

## 5. Commit Code & Create PR

### 1. Pull Latest Code

```
git pull origin main
```

### 2. Create a New Branch

```
git checkout -b feature/my-feature
```

> [!WARNING]
> Avoid developing directly on `main` branch

### 3. Pre-Commit Checks

- Ensure code style is correct
- All unit tests pass
- No useless files

### 4. Commit and Push

```
git status
git add <related-file-paths>
git diff --cached --stat
git commit -m "feat: add my feature"
git push origin feature/my-feature
```

### 5. Create a PR

- Choose `main` as the target branch
- Briefly describe the feature or changes
- Link related Issue if available

> [!TIP]
>
> - Keep each commit focused on a single feature or issue; avoid large, messy commits
> - PR descriptions should be clear so reviewers immediately understand the changes
