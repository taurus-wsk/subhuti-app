# Git 标签管理指南

> **版本**: v1.0  
> **日期**: 2026-06-28  
> **适用范围**: Subhuti 项目所有版本发布

---

## 📖 什么是 Git 标签？

Git 标签（Tag）是给特定提交打上的**永久标记**，用于标记重要的版本发布点。

**类比理解**：
- 提交历史 = 一本书的所有页面
- **标签 = 书签**，标记重要章节（v1.0、v2.0 等）

---

## 🎯 为什么需要标签？

### 1️⃣ 版本标记

```
v0.1.0 - 初始版本
v0.2.0 - 日志查询增强
v1.0.0 - 第一个正式版
```

一眼就知道哪个提交对应哪个版本。

### 2️⃣ 快速定位

```bash
git show v0.2.0          # 查看 v0.2.0 的内容
git diff v0.1.0 v0.2.0   # 对比两个版本差异
```

### 3️⃣ 发布管理

- **Docker 镜像标签**：`subhuti:v0.2.0`
- **GitHub Releases**：自动生成发布页面
- **回滚**：`git checkout v0.1.0`

### 4️⃣ 团队协作

> "这个 bug 在 v0.2.0 修复了"  
> "请升级到 v0.3.0"

---

## 📝 标签的类型

### 1. 轻量标签（Lightweight Tag）

```bash
git tag v0.1.0
```

- 只是指向提交的指针
- 不包含额外信息
- **适合**：临时标记

### 2. 附注标签（Annotated Tag）⭐ 推荐

```bash
git tag -a v0.2.0 -m "日志查询 API 增强"
```

- 包含完整信息（标签名、邮箱、日期、说明）
- 可以 GPG 签名验证
- **适合**：正式发布

---

## 🔧 常用操作

### 创建标签

```bash
# 附注标签（推荐）
git tag -a v0.2.0 -m "Release v0.2.0: 日志查询 API 增强"

# 轻量标签
git tag v0.2.0
```

### 查看标签

```bash
# 列出所有标签
git tag
git tag -l "v0.*"          # 过滤

# 查看标签详情
git show v0.2.0

# 查看当前提交的标签
git describe --tags
```

### 推送标签

```bash
# 推送单个标签
git push origin v0.2.0

# 推送所有标签
git push origin --tags
```

### 删除标签

```bash
# 删除本地标签
git tag -d v0.2.0

# 删除远程标签
git push origin --delete v0.2.0
```

### 检出标签

```bash
# 切换到标签版本（只读）
git checkout v0.2.0

# 基于标签创建分支
git checkout -b fix/v0.2.0-hotfix v0.2.0
```

---

## 📌 语义化版本规范（SemVer）

### 格式：`MAJOR.MINOR.PATCH`

| 部分 | 含义 | 示例 |
|------|------|------|
| **MAJOR** | 不兼容的 API 修改 | v**1**.0.0 → v**2**.0.0 |
| **MINOR** | 向下兼容的功能新增 | v0.**1**.0 → v0.**2**.0 |
| **PATCH** | 向下兼容的问题修正 | v0.2.**0** → v0.2.**1** |

### 版本演进示例

```
v0.1.0 - 初始版本
v0.1.1 - 修复 bug
v0.1.2 - 修复另一个 bug
v0.2.0 - 新增功能（日志查询增强）
v0.2.1 - 修复 v0.2.0 的 bug
v0.3.0 - 新增更多功能
v1.0.0 - 第一个稳定版
v1.1.0 - 新增功能
v2.0.0 - API 破坏性变更
```

### 特殊版本

- **v0.x.x**：开发中版本，API 可能不稳定
- **v1.0.0**：第一个正式稳定版
- **vx.x.x-beta**：测试版
- **vx.x.x-rc**：候选发布版

---

## 🎯 标签说明编写规范

### 格式

```
<简短描述（50 字符内）>

<空行>

<详细说明（分类列出）>
```

### 示例

```bash
git tag -a v0.2.0 -m "feat: 日志查询 API 增强

## 新增功能
- 时间范围过滤（start/end 参数）
- 日志级别过滤（level 参数）
- 分页支持（page/page_size 参数）

## 技术改进
- 优化 read_logs 函数参数封装
- 创建 LogFilterParams 结构体

## 修复问题
- 修复 Clippy \"too many arguments\" 警告
- 修复时间比较类型不匹配错误"
```

### 分类前缀

- **feat**: 新功能
- **fix**: 修复 bug
- **perf**: 性能优化
- **refactor**: 重构
- **docs**: 文档更新
- **chore**: 构建/工具链变更

---

## 🚀 在标准流程中的使用

### 阶段 5: 发布部署 - 步骤 4

```bash
# 1. 提交所有更改
git add .
git commit -m "chore: release v0.2.0"

# 2. 打标签
git tag -a v0.2.0 -m "Release v0.2.0: 日志查询 API 增强

## 新增功能
- 时间范围过滤
- 级别过滤
- 分页支持"

# 3. 验证标签
git tag -l                    # 查看本地标签
git show v0.2.0               # 查看详情

# 4. 推送到 GitHub
git push origin main
git push origin v0.2.0        # 推送标签

# 5. GitHub 自动创建 Release 页面
# 访问：https://github.com/<user>/<repo>/releases/tag/v0.2.0
```

---

## 🔗 标签与 Docker 镜像

### 自动获取版本号

```bash
# 从 Git 标签获取版本
VERSION=$(git describe --tags --exact-match 2>/dev/null || \
          git describe --tags --abbrev=0 2>/dev/null || \
          echo "dev")

echo "📦 构建版本: $VERSION"
# 输出：📦 构建版本: v0.2.0
```

### 构建 Docker 镜像

```bash
# 使用版本号构建
docker build -t subhuti:$VERSION .

# 标记 latest（仅正式版本）
if [[ $VERSION == v* ]]; then
    docker tag subhuti:$VERSION subhuti:latest
fi

# 验证
docker images subhuti
```

### 镜像标签规范

- **正式版本**：`subhuti:v0.2.0`
- **最新版本**：`subhuti:latest`
- **开发版本**：`subhuti:dev`

---

## 📊 标签与测试脚本

### 增强版测试脚本（自动获取标签）

```bash
# 运行测试（自动显示版本信息）
./scripts/test/version-test-enhanced.sh full

# 输出示例：
# ╔═══════════════════════════════════════════════════════════╗
# ║         Subhuti 版本测试 - v0.2.0
# ╚═══════════════════════════════════════════════════════════╝
#
# 📦 版本信息
#   ✅ Git 标签: v0.2.0
#   当前提交: e9d355e
#   分支: main
#   测试时间: 2026-06-28 15:30:00
```

### 测试报告包含版本信息

```
📊 测试报告
  版本: v0.2.0
  总计: 14 项测试
  通过: 14
  失败: 0

✅ 所有测试通过！
```

---

## 💡 实际使用场景

### 场景 1：发布新版本

```bash
# 1. 完成开发，通过测试
make release-test

# 2. 打标签
git tag -a v0.2.0 -m "feat: 日志查询 API 增强"

# 3. 推送
git push origin main
git push origin v0.2.0

# 4. 构建 Docker
docker build -t subhuti:v0.2.0 .

# 5. GitHub 自动生成 Release
# 编辑 Release 说明，添加 changelog
```

### 场景 2：回滚到旧版本

```bash
# 发现 v0.2.0 有严重 bug
# 回滚到 v0.1.0

git checkout v0.1.0

# 或创建修复分支
git checkout -b fix/v0.1.0-hotfix v0.1.0
```

### 场景 3：查看版本差异

```bash
# 对比 v0.1.0 和 v0.2.0 的变化
git diff v0.1.0 v0.2.0

# 查看新增的文件
git diff --name-status v0.1.0 v0.2.0

# 统计变更
git shortlog v0.1.0..v0.2.0
```

### 场景 4：GitHub Releases

推送标签后，GitHub 会自动创建 Release：

```bash
git push origin v0.2.0
```

然后访问：`https://github.com/<user>/<repo>/releases/tag/v0.2.0`

可以编辑：
- Release 标题
- 详细说明（Markdown）
- 附加二进制文件
- 标记为 Pre-release 或 Latest

---

## 🔍 常见问题

### Q1: 标签可以修改吗？

**A**: 不建议修改已推送的标签。如果必须修改：

```bash
# 删除本地和远程标签
git tag -d v0.2.0
git push origin --delete v0.2.0

# 重新创建
git tag -a v0.2.0 -m "新说明"
git push origin v0.2.0
```

### Q2: 忘记推送标签怎么办？

**A**: 随时可以补推：

```bash
git push origin v0.2.0
# 或推送所有未推送的标签
git push origin --tags
```

### Q3: 如何在 CI/CD 中使用标签？

**A**: GitHub Actions 示例：

```yaml
on:
  push:
    tags:
      - 'v*'

jobs:
  release:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      
      - name: Get version
        run: echo "VERSION=${GITHUB_REF#refs/tags/}" >> $GITHUB_ENV
      
      - name: Build Docker
        run: docker build -t subhuti:${{ env.VERSION }} .
```

### Q4: 标签和分支的区别？

**A**: 
- **分支**：会移动，指向最新提交
- **标签**：固定不动，永久标记某个提交

```
提交历史：A → B → C → D → E
                         ↑
                      main (分支，会移动)
                   ↑
                v0.2.0 (标签，固定)
```

---

## 📋 检查清单

发布版本前，确认：

- [ ] 代码已通过所有测试
- [ ] 文档已更新
- [ ] CHANGELOG.md 已更新
- [ ] Cargo.toml 版本号已更新
- [ ] 标签说明已按规范编写
- [ ] 已本地测试标签
- [ ] 已推送代码和标签
- [ ] GitHub Release 已编辑
- [ ] Docker 镜像已构建

---

## 📚 相关文档

- [标准开发流程手册](./STANDARD_WORKFLOW.md)
- [AI 快速参考](./AI_QUICK_REFERENCE.md)
- [版本测试脚本](../../scripts/test/version-test-enhanced.sh)

---

**最后更新**: 2026-06-28  
**维护者**: Subhuti Team
