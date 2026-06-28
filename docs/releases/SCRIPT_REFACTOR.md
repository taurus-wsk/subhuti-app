# 脚本目录重构说明

> **日期**: 2026-06-28  
> **目的**: 整理脚本文件，提高项目可维护性

---

## 📋 变更内容

### 之前（混乱）

```
项目根目录/
├── dev.sh                    # 开发脚本
├── docker.sh                 # Docker 脚本
├── release-test.sh           # 发布测试
├── version-test-template.sh  # 版本测试模板
├── version-test-v0.2.0.sh    # 版本测试
├── test_expert.sh            # 专家测试
├── test_expert_v2.sh         # 专家 V2 测试
└── scripts/
    └── pre-commit            # Git 钩子
```

**问题**：
- ❌ 根目录太乱，脚本散落
- ❌ 没有分类，难以查找
- ❌ 版本测试脚本暴露在根目录

---

### 之后（清晰）

```
项目根目录/
└── scripts/
    ├── README.md             # 脚本目录说明
    ├── build/                # 构建和部署脚本
    │   ├── dev.sh            # 本地开发环境管理
    │   └── docker.sh         # Docker 容器管理
    │
    ├── test/                 # 测试脚本
    │   ├── version-test-template.sh   # 版本测试模板
    │   ├── version-test-v0.2.0.sh     # v0.2.0 版本测试
    │   ├── test_expert.sh             # 专家插件测试
    │   └── test_expert_v2.sh          # 专家 V2 测试
    │
    ├── release/              # 发布脚本
    │   └── release-test.sh            # 发布验证（14 项测试）
    │
    └── pre-commit            # Git 钩子脚本
```

**改进**：
- ✅ 根目录清爽
- ✅ 脚本按功能分类
- ✅ 版本测试脚本归入 test/ 目录
- ✅ 每个目录有明确用途

---

## 🔄 路径变更

### Makefile 更新

| 之前 | 之后 |
|------|------|
| `./dev.sh start` | `./scripts/build/dev.sh start` |
| `./docker.sh build` | `./scripts/build/docker.sh build` |
| `./release-test.sh` | `./scripts/release/release-test.sh` |

### 文档更新

所有文档中的脚本路径已更新：
- ✅ `docs/releases/STANDARD_WORKFLOW.md`
- ✅ `docs/releases/AI_QUICK_REFERENCE.md`
- ✅ `docs/releases/README.md`
- ✅ `docs/releases/INDEX.md`

---

## 📁 分类说明

### build/ - 构建和部署

**用途**：项目构建、服务启动、容器管理

**包含**：
- `dev.sh` - 本地开发服务器管理
- `docker.sh` - Docker 容器生命周期管理

**使用场景**：
- 日常开发启动服务
- Docker 构建和部署
- 查看日志和状态

---

### test/ - 测试脚本

**用途**：各种测试脚本，包括版本测试、功能测试

**包含**：
- `version-test-template.sh` - 版本测试模板
- `version-test-vX.X.0.sh` - 特定版本测试
- `test_expert.sh` - 专家插件测试
- `test_expert_v2.sh` - 专家 V2 测试

**使用场景**：
- 创建新版本时复制模板
- 开发中运行版本测试
- 功能专项测试

---

### release/ - 发布脚本

**用途**：发版本前的验证流程

**包含**：
- `release-test.sh` - 14 项全量验证

**使用场景**：
- 每次发版本前运行
- 生成测试报告
- 验证 Docker 构建

---

## 🚀 使用指南

### 日常开发

```bash
# 启动服务（不变，通过 Makefile）
make serve

# 或直接调用
./scripts/build/dev.sh start
```

### 版本测试

```bash
# 创建新版本测试脚本
cp scripts/test/version-test-template.sh scripts/test/version-test-vX.X.0.sh
chmod +x scripts/test/version-test-vX.X.0.sh

# 运行测试
./scripts/test/version-test-vX.X.0.sh quick
```

### 发布验证

```bash
# 通过 Makefile（推荐）
make release-test

# 或直接调用
./scripts/release/release-test.sh
```

### Docker 部署

```bash
# 通过 Makefile（推荐）
make docker

# 或直接调用
./scripts/build/docker.sh build
./scripts/build/docker.sh start
```

---

## ✅ 验证清单

- [x] 所有脚本已移动到新目录
- [x] Makefile 路径已更新
- [x] 文档路径已更新
- [x] 脚本执行权限正确
- [x] `make help` 正常显示
- [x] `make serve` 正常工作
- [x] `make release-test` 路径正确

---

## 📝 维护说明

### 添加新脚本

1. **构建脚本** → 放入 `scripts/build/`
2. **测试脚本** → 放入 `scripts/test/`
3. **发布脚本** → 放入 `scripts/release/`
4. 添加执行权限：`chmod +x scripts/category/script.sh`
5. 更新 `scripts/README.md`

### 版本测试脚本

新版本创建：

```bash
# 1. 复制模板
cp scripts/test/version-test-template.sh scripts/test/version-test-vX.X.0.sh

# 2. 添加执行权限
chmod +x scripts/test/version-test-vX.X.0.sh

# 3. 编辑测试内容
vim scripts/test/version-test-vX.X.0.sh
```

### 更新脚本

1. 编辑对应脚本
2. 测试更改
3. 更新 `scripts/README.md`（如需要）
4. 提交：`git commit -m "chore: update script name"`

---

## 🎯 优势

### 1. 清晰的目录结构

```
scripts/
├── build/    → 构建相关
├── test/     → 测试相关
└── release/  → 发布相关
```

一目了然，快速定位。

### 2. 根目录整洁

之前：8 个脚本散落在根目录  
之后：0 个脚本在根目录

### 3. 易于维护

- 按功能分类，职责清晰
- 添加新脚本有明确位置
- 旧脚本可归档到 `scripts/archive/`

### 4. 向后兼容

- Makefile 命令不变（`make serve` 等）
- 直接调用路径更新
- 文档已全部更新

---

## 📚 相关文档

- [脚本目录说明](../../scripts/README.md)
- [标准流程手册](./STANDARD_WORKFLOW.md)
- [AI 快速参考](./AI_QUICK_REFERENCE.md)
- [文档索引](./INDEX.md)

---

**完成日期**: 2026-06-28  
**执行人**: AI Assistant  
**状态**: ✅ 已完成并验证
