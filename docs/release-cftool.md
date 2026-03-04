# 发布 cftool 到 PyPI

cftool 由 Rust 编写，通过 [maturin](https://maturin.rs/) 打包为 Python 可安装的二进制 wheel，依赖已发布到 PyPI 的 `cf-extractor`。用户可通过 `pip install cftool` 或 `uv tool install cftool` 安装，获得 `cftool` 命令。

**前置**：请先完成 [cf-extractor 的 PyPI 发布](release-cf-extractor.md)，确保 `pip install cf-extractor` 可用，否则 cftool 的依赖无法解析。

## 前置条件

1. **PyPI 账号**、**API Token**：同 cf-extractor（若已配置 GitHub Environments，可与 cf-extractor 共用 test / prod 两个 Environment 及其中的 `PYPI_API_TOKEN`）。
2. **本地环境**：Rust 工具链、Python 3.9+、maturin（`pip install maturin` 或 `uv tool install maturin`）。
3. **版本号**：发布前在仓库根目录的 `pyproject.toml` 和 `Cargo.toml` 中统一版本号（两处建议一致，如 `0.1.0`）。

## 方式一：本地手动发布

### 1. 统一并更新版本号

编辑仓库根目录：

- `pyproject.toml` → `version = "0.1.0"`
- `Cargo.toml` → `version = "0.1.0"`

按语义化版本递增（如 `0.1.1`）。

### 2. 运行测试

```bash
cargo test --lib
cargo clippy -- -D warnings
```

### 3. 构建 wheel（当前平台）

```bash
pip install maturin
maturin build --release -o dist --compatibility pypi
```

生成的 wheel 在 `dist/` 下，仅适用于当前操作系统/架构。若要覆盖多平台，需在对应系统上分别构建（或使用 CI）。

### 4. 上传到 TestPyPI（可选）

```bash
pip install twine
twine upload --repository-url https://test.pypi.org/legacy/ dist/*
# 用户名填 __token__，密码填 TestPyPI API token
```

### 5. 上传到正式 PyPI

```bash
twine upload dist/*
# 或使用环境变量：
# export TWINE_USERNAME=__token__
# export TWINE_PASSWORD=pypi-xxxx
# twine upload dist/*
```

### 6. 提交并打 tag（推荐）

```bash
git add pyproject.toml Cargo.toml
git commit -m "chore: release cftool 0.1.0"
git tag cftool/v0.1.0
git push origin main --tags
```

---

## 方式二：GitHub Actions 一键发布（推荐）

仓库中已配置 **Publish cftool to PyPI**（`release-cftool.yml`），会为 **Linux / macOS / Windows** 各构建一个 wheel 并上传到所选 PyPI 环境。

### 配置 GitHub Environments

与 cf-extractor 相同：在仓库 **Settings → Environments** 中确保有 **test** 和 **prod** 两个环境，且各自配置 Environment secret **PYPI_API_TOKEN**（test 用 TestPyPI token，prod 用正式 PyPI token）。若已为 cf-extractor 配好，可直接复用。

### 执行发布

1. 在 **pyproject.toml** 和 **Cargo.toml** 中改好版本号并推送到目标分支（如 `main`）。
2. 打开 **Actions** → 选择 **Publish cftool to PyPI** → **Run workflow**。
3. 选择分支后，在 **Publish to (use GitHub Environment for API key)** 中选 **test** 或 **prod**。
4. 点击 **Run workflow**。

Workflow 会：

- 运行 Rust 测试与 clippy
- 在 Ubuntu / macOS / Windows 上分别用 maturin 构建 wheel
- 将全部 wheel 合并后，用所选 Environment 的 `PYPI_API_TOKEN` 上传到 TestPyPI 或 PyPI

**注意**：首次发布建议先选 **test** 验证安装与运行，再选 **prod** 发布正式版。

---

## 安装与使用（用户侧）

发布成功后，用户可：

```bash
# pip
pip install cftool

# uv
uv tool install cftool
```

运行：

```bash
cftool --help
# 或配合 cf-extractor 使用（cftool 会拉取 cf-extractor 作为依赖）
```

依赖关系：`cftool` 依赖 `cf-extractor`，因此 `pip install cftool` 会自动安装 `cf-extractor`，无需单独安装。
