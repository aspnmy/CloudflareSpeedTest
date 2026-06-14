---
name: "release-workflow"
description: "生成/维护 GitHub Actions Release 工作流，支持 Rust 项目的多平台交叉编译、tag 触发、Release 资产上传。在用户要求编写、修复或改进 release.yml 时调用。"
---

# Release Workflow — GitHub Actions 自动发布工作流

本技能用于维护 Rust 项目中 `.github/workflows/release.yml` 工作流，把项目构建为多平台的可执行文件并通过 tag 推送触发自动 GitHub Release 与资产上传。

## 适用场景

- 从零创建一个 Rust 项目的 Release 工作流
- 修复现有 release.yml 中的问题（如未生成 Release、路径错误、artifact 命名冲突、权限缺失等）
- 为项目添加/移除支持的目标平台

## 核心业务模型

```
+----------------+     +------------------+     +-------------------+
|  推送 git tag  | --> |  多平台并行构建  | --> |  创建 Release     |
|   (v* 开头)   |     |  (matrix)         |     |  并上传所有资产   |
+----------------+     +------------------+     +-------------------+
                                 |
                          +------+------+
                          |             |
                   Linux (gnu/musl) macOS Windows
                   x86_64 / aarch64 / armv7 / arm / i686
```

### 触发条件

```yaml
on:
  push:
    tags:
      - 'v*'
```

只有当 tag 以 `v` 开头（如 `v2.4.0-rust`、`v1.0.0`）时才触发发布工作流。普通 commit push 不会触发。

### 权限声明

```yaml
permissions:
  contents: write
```

用于向仓库写入 Release 与 Assets。

### Matrix 策略

| 操作系统（runs-on） | target | 构建方式 | 归档格式 |
|---------------------|--------|---------|---------|
| `ubuntu-latest` | `x86_64-unknown-linux-gnu` | cargo（原生） | `tar.gz` |
| `ubuntu-latest` | `x86_64-unknown-linux-musl` | cross | `tar.gz` |
| `ubuntu-latest` | `i686-unknown-linux-musl` | cross | `tar.gz` |
| `ubuntu-latest` | `aarch64-unknown-linux-gnu` | cross | `tar.gz` |
| `ubuntu-latest` | `aarch64-unknown-linux-musl` | cross | `tar.gz` |
| `ubuntu-latest` | `armv7-unknown-linux-gnueabihf` | cross | `tar.gz` |
| `ubuntu-latest` | `arm-unknown-linux-gnueabihf` | cross | `tar.gz` |
| `macos-latest` | `x86_64-apple-darwin` | cargo（原生） | `tar.gz` |
| `macos-latest` | `aarch64-apple-darwin` | cargo（原生） | `tar.gz` |
| `windows-latest` | `x86_64-pc-windows-msvc` | cargo（原生） | `zip` |
| `windows-latest` | `i686-pc-windows-msvc` | cargo（原生） | `zip` |

关键原则：**`fail-fast: false`**，任何单个平台构建失败不得阻断其他平台的发布。

## 必须遵循的规则清单

1. **GITHUB_TOKEN 必须显式注入** —— `softprops/action-gh-release` 必须通过 `env.GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}` 注入，否则会静默失败而不创建 Release。
2. **Artifact 命名必须包含版本信息** —— `name: cfst-${{ github.ref_name }}-${{ matrix.target }}`，否则不同 tag 发布之间会产生名称冲突与覆盖。
3. **统一使用 `-` 作为分隔符** —— artifact 名称、压缩包文件名、path 通配符必须使用相同的 `-` 分隔符，避免 `_` 与 `-` 混用导致匹配失败。
4. **跨平台采用官方预编译 cross 二进制** —— `cross-rs/setup-cross@v1` action 不存在，不要使用；推荐直接从 `https://github.com/cross-rs/cross/releases` 下载 `cross-x86_64-unknown-linux-musl.tar.gz` 并放到 `PATH`。原生构建保留 `cargo build`。
5. **每个步骤显式声明 `shell`** —— Windows 步骤必须 `shell: pwsh`，Linux/macOS 步骤使用 `shell: bash`，避免依赖 runner 默认 shell。
6. **压缩包文件路径必须带引号** —— YAML 中 `dist/*` 等通配符与 `**/*` 必须用单引号/双引号包裹，避免 YAML 解析错误。
7. **Artifact 的 `path` 与实际生成文件名严格匹配** —— 例如生成 `cfst-${{ github.ref_name }}-${{ matrix.target }}.tar.gz`，path 通配符必须写成 `cfst-${{ github.ref_name }}-${{ matrix.target }}.*`。
8. **`fail_on_unmatched_files: true`** —— Release 步骤开启此开关，便于调试时第一时间暴露 asset 匹配失败。
9. **二进制文件路径必须加引号** —— `cp "target/${{ matrix.target }}/release/${{ matrix.bin }}" dist/`，避免 target 中含空格时命令失败。
10. **版本号遵循 SemVer** —— 形如 `v2.4.0-rust`（hyphen 分隔 pre-release 标识），禁止使用下划线 `_` 作为版本分隔符。

## 标准模板（release.yml）

以下是经过实测的标准工作流，可作为创建新 release.yml 的起点。注意将 `<项目二进制名>` 替换为实际的二进制可执行文件名（通常对应 Cargo.toml 中的 `name` 字段）。

```yaml
name: Release

on:
  push:
    tags:
      - 'v*'

permissions:
  contents: write

jobs:
  build:
    name: Build-${{ matrix.target }}
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        include:
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            bin: <项目二进制名>
            archive: tar.gz
            cross: false
          - os: ubuntu-latest
            target: x86_64-unknown-linux-musl
            bin: <项目二进制名>
            archive: tar.gz
            cross: true
          - os: ubuntu-latest
            target: i686-unknown-linux-musl
            bin: <项目二进制名>
            archive: tar.gz
            cross: true
          - os: ubuntu-latest
            target: aarch64-unknown-linux-gnu
            bin: <项目二进制名>
            archive: tar.gz
            cross: true
          - os: ubuntu-latest
            target: aarch64-unknown-linux-musl
            bin: <项目二进制名>
            archive: tar.gz
            cross: true
          - os: ubuntu-latest
            target: armv7-unknown-linux-gnueabihf
            bin: <项目二进制名>
            archive: tar.gz
            cross: true
          - os: ubuntu-latest
            target: arm-unknown-linux-gnueabihf
            bin: <项目二进制名>
            archive: tar.gz
            cross: true
          - os: macos-latest
            target: x86_64-apple-darwin
            bin: <项目二进制名>
            archive: tar.gz
            cross: false
          - os: macos-latest
            target: aarch64-apple-darwin
            bin: <项目二进制名>
            archive: tar.gz
            cross: false
          - os: windows-latest
            target: x86_64-pc-windows-msvc
            bin: <项目二进制名>.exe
            archive: zip
            cross: false
          - os: windows-latest
            target: i686-pc-windows-msvc
            bin: <项目二进制名>.exe
            archive: zip
            cross: false
    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - name: Setup cross (cross-compilation)
        if: matrix.cross == true
        shell: bash
        run: |
          curl -fsSL https://github.com/cross-rs/cross/releases/latest/download/cross-x86_64-unknown-linux-musl.tar.gz | tar -xz -C /tmp
          mv /tmp/cross /usr/local/bin/cross
          cross --version

      - name: Build (native)
        if: matrix.cross == false
        shell: bash
        run: cargo build --release --target ${{ matrix.target }}

      - name: Build (cross)
        if: matrix.cross == true
        shell: bash
        run: cross build --release --target ${{ matrix.target }}

      - name: Prepare artifact directory
        shell: bash
        run: |
          mkdir -p dist
          cp "target/${{ matrix.target }}/release/${{ matrix.bin }}" dist/
          if [ -f README.md ]; then cp README.md dist/; fi
          if [ -f LICENSE ]; then cp LICENSE dist/; fi

      - name: Create tar.gz archive
        if: matrix.archive == 'tar.gz'
        shell: bash
        run: |
          cd dist
          tar -czf "../<项目二进制名>-${{ github.ref_name }}-${{ matrix.target }}.tar.gz" .
          cd ..
          ls -la "<项目二进制名>-${{ github.ref_name }}-${{ matrix.target }}.tar.gz"

      - name: Create zip archive
        if: matrix.archive == 'zip'
        shell: pwsh
        run: |
          Compress-Archive -Path "dist/*" -DestinationPath "<项目二进制名>-${{ github.ref_name }}-${{ matrix.target }}.zip"
          Get-Item "<项目二进制名>-${{ github.ref_name }}-${{ matrix.target }}.zip"

      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: <项目二进制名>-${{ github.ref_name }}-${{ matrix.target }}
          path: <项目二进制名>-${{ github.ref_name }}-${{ matrix.target }}.*
          compression-level: 0
          retention-days: 7

  release:
    name: Create GitHub Release
    needs: build
    runs-on: ubuntu-latest
    steps:
      - name: Download all build artifacts
        uses: actions/download-artifact@v4
        with:
          path: artifacts

      - name: Display artifacts
        run: ls -R artifacts/

      - name: Create Release and upload assets
        uses: softprops/action-gh-release@v2
        with:
          files: 'artifacts/**/*'
          generate_release_notes: true
          prerelease: false
          fail_on_unmatched_files: true
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

## 常见故障排查（Issue Triage）

| 现象 | 可能原因 | 修复 |
|------|---------|-----|
| 构建全部成功但 GitHub 没有 Release 页 | 缺少 `env.GITHUB_TOKEN` 或 `permissions: contents: write` | 同时补上两者 |
| Release 生成但没有任何 Assets | `files` 通配符与 artifact 路径不匹配 | 检查 download-artifact 目录结构并修正通配符 |
| Windows 步骤报路径解析错误 | 未显式 `shell: pwsh` 或 `dist/*` 写成 `dist\\*` | 改为正斜杠并加引号 |
| artifact 上传失败（duplicate / 不匹配） | artifact `name` 跨 matrix 不唯一 | 必须包含 `github.ref_name` 与 `matrix.target` |
| cross 安装/编译失败 | `cargo install cross` 太慢或依赖失败 | 改用官方 releases 页面下载 `cross-x86_64-unknown-linux-musl.tar.gz` 直接解压到 `PATH` |
| Release 步骤失败但日志信息不足 | `fail_on_unmatched_files` 未开启 | 设为 `true` 便于定位匹配问题 |

## 触发发布

在已合入 release.yml 的分支上执行以下命令以触发一次发布：

```bash
# 读取 Cargo.toml 中 version 字段作为 tag
VERSION=$(grep '^version' Cargo.toml | head -1 | sed -E 's/version[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/')
git tag "v${VERSION}"
git push origin "v${VERSION}"
```

## 输出物清单

- GitHub Release 页（自动生成的 release notes）
- 每个 target 一个压缩包：`<bin>-v<版本>-<target>.tar.gz` 或 `.zip`
- 每个 target 一个 workflow run artifact（保留 7 天）
