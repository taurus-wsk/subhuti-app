# Subhuti 工程化体系文件清单

> **更新日期**: 2026-06-28  
> **用途**: 快速定位所有工程化相关文件

---

## 📋 文档体系

### 核心流程文档（必读）

| 文件 | 路径 | 用途 | 读者 |
|------|------|------|------|
| **标准流程手册** | `docs/releases/STANDARD_WORKFLOW.md` | 完整的五阶段开发流程 | 所有开发者、AI |
| **AI 快速参考** | `docs/releases/AI_QUICK_REFERENCE.md` | AI Assistant 快速指令卡 | AI Assistant |
| **版本迭代指南** | `docs/releases/README.md` | 快速上手指引 | 新成员 |

### 文档模板

| 模板 | 路径 | 用途 | 使用时机 |
|------|------|------|---------|
| **需求规格** | `docs/releases/TEMPLATE_REQUIREMENTS.md` | 定义版本需求 | 需求分析阶段 |
| **概要设计** | `docs/releases/TEMPLATE_DESIGN.md` | 技术方案设计 | 设计评审阶段 |
| **测试用例** | `docs/releases/TEMPLATE_TEST_CASES.md` | 详细测试用例 | 测试准备阶段 |
| **发布清单** | `docs/releases/TEMPLATE_RELEASE_CHECKLIST.md` | 发布检查项 | 发布准备阶段 |

### 参考文档

| 文档 | 路径 | 用途 |
|------|------|------|
| **版本迭代流程** | `docs/releases/VERSION_WORKFLOW.md` | 完整流程说明 |
| **脚本工具参考** | `docs/SCRIPTS_REFERENCE.md` | 所有脚本工具用法 |
| **架构文档** | `docs/ARCHITECTURE.md` | 系统架构说明 |
| **API 教程** | `docs/API_TUTORIAL.md` | API 使用教程 |
| **用户指南** | `docs/USER_GUIDE.md` | 用户使用指南 |

---

## 🛠️ 脚本工具体系

> **所有脚本已分类整理**，详见 [scripts/README.md](../../scripts/README.md)

### 脚本目录结构

```
scripts/
├── build/          # 构建和部署脚本
│   ├── dev.sh      # 本地开发环境管理
│   └── docker.sh   # Docker 容器管理
│
├── test/           # 测试脚本
│   ├── version-test-template.sh   # 版本测试模板
│   ├── version-test-v0.2.0.sh     # v0.2.0 版本测试
│   ├── test_expert.sh             # 专家插件测试
│   └── test_expert_v2.sh          # 专家 V2 测试
│
├── release/        # 发布脚本
│   └── release-test.sh            # 发布验证（14 项测试）
│
└── pre-commit      # Git 钩子脚本
```

### 核心脚本

| 脚本 | 用途 | 使用场景 |
|------|------|---------|
| **Makefile** | 统一命令入口 | 日常开发、构建、测试 |
| **dev.sh** | 本地开发管理 | 启动、停止、查看日志 |
| **docker.sh** | Docker 容器管理 | 构建、部署、运维 |

### 测试脚本

| 脚本 | 用途 | 使用场景 |
|------|------|---------|
| **release-test.sh** | 发布验证（14 项） | 每次发版本前 |
| **version-test-template.sh** | 版本测试模板 | 创建新版本时复制 |
| **version-test-v0.2.0.sh** | v0.2.0 版本测试 | v0.2.0 专属 |
| **test_expert.sh** | 专家插件测试 | 测试专家系统 |
| **test_expert_v2.sh** | 专家 V2 测试 | 测试专家 V2 |

### 自动化脚本

| 脚本 | 用途 | 触发时机 |
|------|------|---------|
| **scripts/pre-commit** | Git 钩子 | 每次 git commit |

---

## 📁 配置文件

| 文件 | 用途 |
|------|------|
| **config/Subhuti.toml** | TOML 配置文件 |
| **Cargo.toml** | Rust 工作区配置 |
| **.env** | 环境变量（不提交） |
| **Dockerfile** | Docker 镜像构建 |

---

## 📦 版本文档示例

### v0.1.0（初始版本）

```
docs/releases/v0.1.0/
└── REQUIREMENTS.md  # 需求规格（示例）
```

### v0.2.0（日志查询增强）

```
docs/releases/v0.2.0/
├── REQUIREMENTS.md           # 需求规格
├── DESIGN.md                 # 概要设计
├── TEST_CASES.md             # 测试用例
├── PROCESS_VERIFICATION.md   # 流程验证报告
└── RELEASE_CHECKLIST.md      # 发布清单（待填写）
```

---

## 🎯 使用指南

### 新成员入职

1. 阅读 `docs/releases/README.md` - 快速了解
2. 阅读 `docs/releases/STANDARD_WORKFLOW.md` - 掌握流程
3. 阅读 `docs/SCRIPTS_REFERENCE.md` - 了解工具
4. 查看 `docs/releases/v0.2.0/` - 参考实例

### AI Assistant 开发

1. 阅读 `docs/releases/AI_QUICK_REFERENCE.md` - 快速指令
2. 参考 `docs/releases/STANDARD_WORKFLOW.md` - 详细流程
3. 查看 `docs/releases/v0.2.0/` - 实际案例

### 创建新版本

```bash
# 1. 创建目录
mkdir -p docs/releases/vX.X.0

# 2. 复制模板
cp docs/releases/TEMPLATE_*.md docs/releases/vX.X.0/

# 3. 创建测试脚本
cp version-test-template.sh version-test-vX.X.0.sh
chmod +x version-test-vX.X.0.sh

# 4. 开始开发（遵循标准流程）
```

### 发版本

```bash
# 1. 运行检查
make check

# 2. 运行测试
./version-test-vX.X.0.sh full
make release-test

# 3. 查看报告
cat RELEASE_TEST_REPORT.md

# 4. 发布
git tag -a vX.X.0 -m "Release vX.X.0"
git push origin vX.X.0
```

---

## 📊 文档完整性检查

### 版本发布前必须包含

- [ ] `REQUIREMENTS.md` - 需求规格
- [ ] `DESIGN.md` - 概要设计
- [ ] `TEST_CASES.md` - 测试用例
- [ ] `RELEASE_CHECKLIST.md` - 发布清单
- [ ] `version-test-vX.X.0.sh` - 版本测试脚本

### 可选文档

- [ ] `PROCESS_VERIFICATION.md` - 流程验证报告
- [ ] 性能测试报告
- [ ] 用户手册更新

---

## 🔄 维护说明

### 更新模板

当发现模板需要改进时：

1. 编辑 `TEMPLATE_*.md` 文件
2. 更新版本号和维护日期
3. 提交更改：`git commit -m "docs: update template"`

### 添加新脚本

1. 在根目录创建 `.sh` 文件
2. 添加执行权限：`chmod +x script.sh`
3. 在 Makefile 添加 target（如适用）
4. 更新 `docs/SCRIPTS_REFERENCE.md`

### 版本文档归档

旧版本文档保留在 `docs/releases/vX.X.0/` 目录，不删除。

---

## 📈 工程化成熟度

### 已实现 ✅

- [x] 标准化开发流程（五阶段）
- [x] 文档模板体系（4 个模板）
- [x] 自动化测试（版本测试 + 发布验证）
- [x] 代码质量门禁（pre-commit）
- [x] 配置管理系统（TOML）
- [x] Docker 容器化
- [x] 统一命令入口（Makefile）
- [x] 完整文档体系

### 规划中 📋

- [ ] CI/CD 流水线（GitHub Actions）
- [ ] API 文档自动生成（OpenAPI）
- [ ] 性能监控面板
- [ ] 自动化变更日志
- [ ] 文档检查脚本

---

## 🎓 学习路径

### 初级（1-2 小时）

1. 阅读 `docs/releases/AI_QUICK_REFERENCE.md`
2. 了解五阶段流程
3. 掌握常用命令

### 中级（半天）

1. 阅读 `docs/releases/STANDARD_WORKFLOW.md`
2. 查看 `v0.2.0` 完整案例
3. 实践一次小功能开发

### 高级（1 天）

1. 理解所有模板设计
2. 掌握故障排查
3. 能够优化流程

---

## 📞 支持

### 问题排查

1. 查看 `docs/SCRIPTS_REFERENCE.md` 故障排查章节
2. 查看 `STANDARD_WORKFLOW.md` 常见问题
3. 查看日志：`./dev.sh logs` 或 `./docker.sh logs`

### 反馈建议

- 提交 Issue
- 联系维护者
- 提交 Pull Request

---

**维护者**: Subhuti Team  
**最后更新**: 2026-06-28  
**文档版本**: v1.0
