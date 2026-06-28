# Subhuti 脚本工具目录

> 所有脚本按功能分类存放，便于查找和维护。

---

## 📁 目录结构

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

---

## 🔨 build/ - 构建和部署脚本

### dev.sh - 本地开发环境管理

**用途**：管理本地开发服务器

**命令**：
```bash
./scripts/build/dev.sh [command]

命令:
  build    编译 release 版本
  start    启动服务（默认）
  stop     停止服务
  restart  重启服务
  status   查看运行状态
  logs     查看实时日志
  test     运行健康检查
```

**示例**：
```bash
# 启动服务
./scripts/build/dev.sh start

# 查看日志
./scripts/build/dev.sh logs

# 健康检查
./scripts/build/dev.sh test
```

---

### docker.sh - Docker 容器管理

**用途**：管理 Docker 容器生命周期

**命令**：
```bash
./scripts/build/docker.sh [command]

命令:
  build    构建 Docker 镜像
  start    启动容器（默认）
  stop     停止并移除容器
  restart  重启容器
  status   查看容器状态
  logs     查看容器日志
```

**示例**：
```bash
# 构建镜像
./scripts/build/docker.sh build

# 启动容器
./scripts/build/docker.sh start

# 查看日志
./scripts/build/docker.sh logs
```

---

## 🧪 test/ - 测试脚本

### version-test-template.sh - 版本测试模板

**用途**：创建新版本测试脚本的模板

**使用**：
```bash
# 复制模板
cp scripts/test/version-test-template.sh scripts/test/version-test-vX.X.0.sh
chmod +x scripts/test/version-test-vX.X.0.sh

# 编辑测试内容
vim scripts/test/version-test-vX.X.0.sh

# 运行测试
./scripts/test/version-test-vX.X.0.sh [full|quick|api|perf]
```

**测试模式**：
- `full` - 完整测试（发布前）
- `quick` - 快速测试（开发中）
- `api` - API 回归测试
- `perf` - 性能测试

---

### version-test-v0.2.0.sh - v0.2.0 版本测试

**用途**：v0.2.0 版本专属测试脚本

**测试内容**：
- 日志级别过滤
- 时间范围查询
- 分页功能
- 基础功能验证

**运行**：
```bash
./scripts/test/version-test-v0.2.0.sh quick
./scripts/test/version-test-v0.2.0.sh full
```

---

### test_expert.sh - 专家插件测试

**用途**：测试专家插件系统的激活、停用、匹配功能

**运行**：
```bash
# 确保服务已启动
./scripts/build/dev.sh start

# 运行测试
./scripts/test/test_expert.sh
```

**测试内容**：
- 专家列表
- 专家激活/停用
- Persona 覆盖
- 技能注入
- 专家匹配

---

### test_expert_v2.sh - 专家 V2 测试

**用途**：测试专家 V2 版本的规划执行链路

**运行**：
```bash
./scripts/test/test_expert_v2.sh
```

---

## 🚀 release/ - 发布脚本

### release-test.sh - 发布验证

**用途**：发版本前的全量验证（14 项测试）

**验证流程**：
1. 代码质量检查（fmt + clippy）
2. Release 编译
3. 单元测试
4. Docker 构建
5. 容器启动
6. API 功能测试（7 项）
7. 清理

**运行**：
```bash
./scripts/release/release-test.sh
# 或
make release-test
```

**输出**：
- 终端显示测试结果
- 生成 `RELEASE_TEST_REPORT.md` 报告

---

## 🔧 pre-commit - Git 钩子

**用途**：提交代码前自动运行质量检查

**安装**：
```bash
make install
# 或
cp scripts/pre-commit .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
```

**检查项**：
1. 代码格式化（cargo fmt --check）
2. Clippy 检查（cargo clippy）
3. 单元测试（cargo test）

---

## 📋 使用场景

### 日常开发

```bash
# 启动服务
./scripts/build/dev.sh start
# 或
make serve

# 查看日志
./scripts/build/dev.sh logs

# 运行测试
./scripts/test/version-test-v0.2.0.sh quick
```

### 代码提交

```bash
# 自动触发 pre-commit
git add .
git commit -m "feat: add feature"
```

### 发版本

```bash
# 运行发布验证
./scripts/release/release-test.sh
# 或
make release-test

# 查看报告
cat RELEASE_TEST_REPORT.md
```

### Docker 部署

```bash
# 构建并启动
./scripts/build/docker.sh build
./scripts/build/docker.sh start
# 或
make docker

# 查看状态
./scripts/build/docker.sh status
```

---

## 🆕 添加新脚本

### 构建脚本

放入 `scripts/build/` 目录：

```bash
# 创建脚本
cat > scripts/build/new-build-script.sh << 'EOF'
#!/bin/bash
# 脚本内容
EOF

# 添加执行权限
chmod +x scripts/build/new-build-script.sh
```

### 测试脚本

放入 `scripts/test/` 目录：

```bash
# 复制模板（版本测试）
cp scripts/test/version-test-template.sh scripts/test/version-test-vX.X.0.sh

# 或直接创建
cat > scripts/test/new-test.sh << 'EOF'
#!/bin/bash
# 测试脚本内容
EOF

chmod +x scripts/test/new-test.sh
```

### 发布脚本

放入 `scripts/release/` 目录：

```bash
cat > scripts/release/new-release-script.sh << 'EOF'
#!/bin/bash
# 发布脚本内容
EOF

chmod +x scripts/release/new-release-script.sh
```

---

## 🔍 快速查找

| 需求 | 脚本 | 路径 |
|------|------|------|
| 启动本地服务 | dev.sh | `scripts/build/` |
| Docker 管理 | docker.sh | `scripts/build/` |
| 版本测试 | version-test-*.sh | `scripts/test/` |
| 专家测试 | test_expert*.sh | `scripts/test/` |
| 发布验证 | release-test.sh | `scripts/release/` |
| Git 钩子 | pre-commit | `scripts/` |

---

## 📝 维护说明

### 更新脚本

1. 编辑对应脚本文件
2. 测试更改
3. 提交：`git commit -m "chore: update script name"`

### 废弃脚本

不要删除，移动到 `scripts/archive/` 目录：

```bash
mkdir -p scripts/archive
mv scripts/test/old-test.sh scripts/archive/
```

### 脚本规范

所有脚本应包含：

```bash
#!/bin/bash
# ============================================================
# 脚本名称
# ============================================================
# 用途：简要说明
# 用法：./script.sh [参数]
# ============================================================
set -e

# 脚本内容
```

---

**最后更新**: 2026-06-28  
**维护者**: Subhuti Team
