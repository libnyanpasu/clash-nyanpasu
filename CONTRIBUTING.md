# Contributing to Nyanpasu

Welcome to **Nyanpasu** development!  
To ensure the quality and stability of the project, please read this guide carefully. Even if you are new, you can follow these steps to set up the development environment, write code, and submit contributions.

---

## 1. Development Guidelines

Before submitting code, please follow these rules:

### 1. Code Style Checks

| Language                | Tools                       |
| ----------------------- | --------------------------- |
| JavaScript / TypeScript | ESLint, Prettier, Stylelint |
| Rust                    | Clippy, Rustfmt             |

- ⚠️ **Ensure there are no style errors before committing**
- ❌ **Do not use `git commit -n` or skip checks**, CI will automatically enforce style validation

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

| Tool    | Version  | Link                                                        | Notes                                         |
| ------- | -------- | ----------------------------------------------------------- | --------------------------------------------- |
| Rust    | ≥ 1.78   | [Official Install](https://www.rust-lang.org/tools/install) | Stable version; use MSVC toolchain on Windows |
| Node.js | ≥ 20 LTS | [Official Site](https://nodejs.org/)                        | Install LTS or Latest version                 |
| pnpm    | ≥ 9      | [Official Documentation](https://pnpm.io/)                  | Node.js package manager                       |
| git     | Latest   | [Official Site](https://git-scm.com/)                       | Version control                               |

### 2. Build Dependencies

| Tool  | Link                                                                              | Notes                               |
| ----- | --------------------------------------------------------------------------------- | ----------------------------------- |
| cmake | [Official Site](https://cmake.org/)                                               | Required by `zip` crate             |
| llvm  | [Official Site](https://llvm.org/)                                                | Required by `rquickjs` or `rocksdb` |
| patch | [Windows Installation Guide](https://gnuwin32.sourceforge.net/packages/patch.htm) | Required by `rquickjs`              |

### 3. Windows Special Requirements

- Use **Administrator privileges** when opening the project for the first time; `patch` requires admin rights
- Recommended to install `gsudo` (via `scoop`, `choco`, or `winget`)
- Always use the **MSVC toolchain** on Windows
- 💡 Admin privileges are only needed for initial setup; normal terminal is fine for daily development

---

## 3. Pre-Development Setup

Before starting development, initialize the environment and download required resources.

### 1. Install Frontend Dependencies

```bash
pnpm i
```

> This installs all frontend dependencies including UI components, toolchains, and testing tools.

### 2. Download Core & Resource Files

```
pnpm check
```

> This command downloads binaries like `sidecar` and `resource` to ensure the project runs properly

If files are missing or you want to force update:

```
pnpm check --force
```

💡 **Tip**: Configure terminal proxy if network issues occur

---

## 4. Start Development Environment

The project provides two types of development instances:

### 1. Dedicated Development Instance (Recommended)

```
pnpm dev:diff
```

> Suitable for daily development and debugging; changes do not affect the release version

### 2. Release-Like Development Instance

```
pnpm dev
```

> Behaves similarly to the official release; useful to test overall functionality

---

## 5. Commit Code & Create PR

### 1. Pull Latest Code

```
git pull origin main
```

### 2. Create a New Branch

```
git checkout -b feature/my-feature
```

> ⚠️ Avoid developing directly on `main`

### 3. Pre-Commit Checks

- Ensure code style is correct
- All unit tests pass
- No useless files

### 4. Commit and Push

```
git add .
git commit -m "feat: add my feature"
git push origin feature/my-feature
```

### 5. Create a PR

- Choose `main` as the target branch
- Briefly describe the feature or changes
- Link related Issue if available

---

💡 **Tips**:

- Keep each commit focused on a single feature or issue; avoid large, messy commits
- PR descriptions should be clear so reviewers immediately understand the changes
