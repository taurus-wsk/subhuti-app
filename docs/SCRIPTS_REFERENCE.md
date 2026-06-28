# Subhuti 脚本工具参考手册

> 本文档汇总了项目中所有的脚本工具和快捷命令，帮助开发者高效使用和运维系统。

---

## 📋 目录

- [Makefile 命令](#makefile-命令)
- [dev.sh - 本地开发](#devsh---本地开发)
- [docker.sh - Docker 管理](#dockersh---docker-管理)
- [release-test.sh - 发布验证](#release-testsh---发布验证)
- [version-test.sh - 版本测试](#version-testsh---版本测试)
- [test_expert.sh - 专家测试](#test_expertsh---专家测试)
- [test_expert_v2.sh - 专家 V2 测试](#test_expert_v2sh---专家-v2-测试)
- [pre-commit 钩子](#pre-commit-钩子)
- [文档体系](#文档体系)

---

## Makefile 命令

Makefile 是项目的统一入口，封装了所有常用操作。

### 快速查看

```bash
make help          # 显示所有可用命令
```

### 开发命令

```bash
make build         # 编译 release 版本
make test          # 运行所有测试
make test-watch    # 监听模式下运行测试
make serve         # 启动 HTTP 服务（后台运行）
```

### Docker 命令

```bash
make docker        # 构建 + 启动 Docker 容器
make docker-build  # 仅构建 Docker 镜像
make docker-stop   # 停止并移除容器
```

### 代码质量

```bash
make fmt           # 格式化代码（cargo fmt）
make clippy        # 运行 clippy 检查
make check         # 完整检查（fmt + clippy + test）
```

### 维护命令

```bash
make clean         # 清理构建产物
make install       # 安装 pre-commit hook
```

### 发布命令

```bash
make release-test  # 发布前全量验证（生成测试报告）
```

**使用建议**：
- 日常开发使用 `make serve` 快速启动
- 提交前运行 `make check` 确保代码质量
- 发版本前执行 `make release-test` 生成测试报告

---

## dev.sh - 本地开发

本地开发环境管理脚本，直接运行编译后的二进制文件。

### 用法

```bash
./dev.sh [command]
```

### 命令列表

| 命令 | 说明 | 示例 |
|------|------|------|
| `build` | 编译 release 版本 | `./dev.sh build` |
| `start` | 启动服务（默认） | `./dev.sh start` |
| `stop` | 停止服务 | `./dev.sh stop` |
| `restart` | 重启服务 | `./dev.sh restart` |
| `status` | 查看运行状态 | `./dev.sh status` |
| `logs` | 查看实时日志 | `./dev.sh logs` |
| `test` | 运行健康检查 | `./dev.sh test` |

### 环境配置

脚本会自动加载以下配置：

```bash
# 数据库配置
DB_HOST=localhost
DB_PORT=5432
DB_DATABASE=postgres
DB_USERNAME=postgres
DB_PASSWORD=123456
DB_MAX_CONN=10

# HTTP 服务
HTTP_ADDR=0.0.0.0:8080

# 日志级别
RUST_LOG=info
```

API Key 等敏感信息从 `.env` 文件加载。

### 使用场景

```bash
# 1. 编译并启动
./dev.sh build
./dev.sh start

# 2. 查看服务状态
./dev.sh status

# 3. 查看实时日志
./dev.sh logs

# 4. 运行健康检查
./dev.sh test

# 5. 重启服务
./dev.sh restart
```

### 文件位置

- 二进制：`target/release/http_server`
- PID 文件：`.http_server.pid`
- 日志目录：`logs/`

---

## docker.sh - Docker 管理

Docker 容器生命周期管理脚本，支持连接宿主机数据库。

### 用法

```bash
./docker.sh [command]
```

### 命令列表

| 命令 | 说明 | 示例 |
|------|------|------|
| `build` | 构建 Docker 镜像 | `./docker.sh build` |
| `start` | 启动容器（默认） | `./docker.sh start` |
| `stop` | 停止并移除容器 | `./docker.sh stop` |
| `restart` | 重启容器 | `./docker.sh restart` |
| `status` | 查看容器状态 | `./docker.sh status` |
| `logs` | 查看容器日志 | `./docker.sh logs` |

### 容器配置

```bash
镜像名称：subhuti
容器名称：subhuti-app
端口映射：8080:8080

# 数据库连接（通过 host.docker.internal 连接宿主机）
DB_HOST=host.docker.internal
DB_PORT=5432
DB_DATABASE=postgres
```

### 使用场景

```bash
# 1. 构建镜像
./docker.sh build

# 2. 启动容器
./docker.sh start

# 3. 查看状态
./docker.sh status

# 4. 查看日志
./docker.sh logs

# 5. 停止容器
./docker.sh stop
```

### 日志挂载

容器日志会挂载到宿主机的 `logs/` 目录：

```bash
# 查看容器日志
./docker.sh logs

# 或直接查看挂载的日志文件
tail -f logs/subhuti.log
```

---

## release-test.sh - 发布验证

发版本前的全量验证脚本，自动化完成从编译到 API 测试的完整流程。

### 用法

```bash
./release-test.sh
# 或
make release-test
```

### 验证流程

| 阶段 | 测试项 | 说明 |
|------|--------|------|
| 1. 代码质量 | 格式化 + Clippy | 确保代码符合规范 |
| 2. Release 编译 | cargo build --release | 验证生产版本编译 |
| 3. 单元测试 | cargo test --workspace | 所有单元测试 |
| 4. Docker 构建 | docker.sh build | 镜像构建 |
| 5. 容器启动 | docker.sh start | 健康检查 |
| 6. API 测试 | 7 个接口 | 健康、技能、专家、人格、Trace、聊天 |
| 7. 清理 | 停止容器 | 清理环境 |

### API 测试覆盖

- ✅ 健康检查：`/subhuti/api/v1/health`
- ✅ 详细状态：`/subhuti/api/v1/health/detailed`
- ✅ 技能列表：`/subhuti/api/v1/skills`
- ✅ 专家列表：`/subhuti/api/v1/experts`
- ✅ 人格信息：`/subhuti/api/v1/persona`
- ✅ Trace 追踪：`/subhuti/api/v1/traces`
- ✅ 聊天功能：`POST /subhuti/api/v1/chat`

### 输出产物

执行完成后会生成测试报告：

```
RELEASE_TEST_REPORT.md
```

报告包含：
- 测试时间、总耗时
- 14 项测试详情
- 环境信息（Rust/Cargo/Docker 版本）
- 发布建议（通过/失败）

### 使用场景

```bash
# 发版本前执行
make release-test

# 查看测试报告
cat RELEASE_TEST_REPORT.md
```

### 执行示例

```
╔══════════════════════════════════════════════════════════╗
║           Subhuti 发布验证流程                           ║
╚══════════════════════════════════════════════════════════╝

阶段 1: 代码质量检查
  ✅ PASS 所有代码格式正确
  ✅ PASS 无警告

阶段 2: Release 编译
  ✅ PASS 耗时 17s, 二进制大小 10M

阶段 3: 单元测试
  ✅ PASS 23 passed

阶段 4: Docker 构建
  ✅ PASS 耗时 19s, 镜像大小 177MB

阶段 5: Docker 容器启动
  ✅ PASS Up 5 seconds (healthy)

阶段 6: API 功能测试
  ✅ PASS 服务正常
  ✅ PASS 5 个组件全部正常
  ✅ PASS 12 个技能
  ✅ PASS 1 个专家
  ✅ PASS 名称: 暖心心理咨询师
  ✅ PASS Trace API 正常
  ✅ PASS 响应: OK..., 耗时 2s

阶段 7: 清理
  ✅ PASS 容器已停止并移除

测试总结
  总计: 14 项
  通过: 14
  失败: 0
  警告: 0
  耗时: 51s

✅ 所有测试通过，可以发布！
📄 测试报告已生成: RELEASE_TEST_REPORT.md
```

---

## test_expert.sh - 专家测试

专家插件激活测试脚本，验证 persona 和知识库是否正确注入。

### 用法

```bash
# 确保服务已启动
./dev.sh start

# 运行测试
./test_expert.sh
```

### 测试步骤

1. 获取专家列表
2. 获取当前激活专家（激活前）
3. 获取当前 persona（激活前）
4. 激活心理咨询专家
5. 验证专家激活成功
6. 验证 persona 被专家覆盖
7. 验证大五人格参数正确
8. 验证专家技能已注入
9. 测试专家匹配功能
10. 停用专家并验证

### 验证内容

- ✅ 专家激活/停用
- ✅ Persona 覆盖（名称、描述、语气）
- ✅ 大五人格参数（宜人性等）
- ✅ 专家技能注入（mood_check、stress_relief）
- ✅ 专家匹配功能

### 输出示例

```
==========================================
  专家插件激活测试
==========================================

【步骤1】获取已注册专家列表...
【步骤4】激活心理咨询专家 (ID: psychology)...
✅ 专家激活成功！当前专家: 心理咨询师
✅ Persona 已被专家覆盖！当前名称: 暖心心理咨询师
✅ 大五人格参数正确！宜人性: 0.9
✅ 专家技能 mood_check 已注入
✅ 专家技能 stress_relief 已注入
✅ 专家匹配正确！输入 '压力很大' 匹配到: 心理咨询师
```

---

## test_expert_v2.sh - 专家 V2 测试

专家 V2 版本测试脚本，测试规划执行链路。

### 用法

```bash
# 确保服务已启动
./dev.sh start

# 运行测试
./test_expert_v2.sh
```

### 测试内容

- 专家规划流程
- 多步骤执行
- 技能调用验证
- 工具调用验证

---

## pre-commit 钩子

Git pre-commit 钩子，自动运行代码质量检查。

### 安装

```bash
make install
# 或手动安装
cp scripts/pre-commit .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
```

### 检查项

| 检查 | 命令 | 说明 |
|------|------|------|
| 代码格式化 | `cargo fmt --check` | 阻止未格式化的代码提交 |
| Clippy | `cargo clippy` | 阻止有警告的代码提交 |
| 单元测试 | `cargo test` | 阻止失败的测试提交 |

### 工作流程

```bash
# 尝试提交代码
git commit -m "feat: add new feature"

# 自动执行检查
🔍 代码格式检查...
  ✅ 通过

🔍 Clippy 检查...
  ✅ 通过

🔍 运行单元测试...
  ✅ 通过

✅ 代码质量检查通过，允许提交
```

### 失败示例

```bash
❌ 代码格式不正确！
   请运行: cargo fmt --all
   修复后重新提交
```

### 跳过检查（不推荐）

```bash
# 紧急情况可跳过（需谨慎）
git commit --no-verify -m "hotfix: urgent fix"
```

---

## version-test.sh - 版本测试

针对特定版本需求的专项测试脚本，每个版本独立维护。

### 用法

```bash
# 复制模板并修改版本号
cp version-test-template.sh version-test-v0.x.0.sh
chmod +x version-test-v0.x.0.sh

# 运行测试
./version-test-v0.x.0.sh [full|quick|api|perf]
```

### 测试模式

| 模式 | 说明 | 使用场景 |
|------|------|---------|
| `full` | 完整测试（默认） | 发布前验证 |
| `quick` | 快速测试 | 日常开发验证 |
| `api` | API 回归测试 | API 修改后 |
| `perf` | 性能测试 | 性能优化后 |

### 测试覆盖

- ✅ 基础功能（健康检查、技能、人格）
- ✅ 新功能（根据版本需求添加）
- ✅ API 回归（聊天、Trace）
- ✅ 性能指标（响应时间）

### 使用场景

```bash
# 开发中快速验证
./version-test-v0.1.0.sh quick

# 发布前完整测试
./version-test-v0.1.0.sh full

# 修改 API 后回归
./version-test-v0.1.0.sh api
```

### 自定义测试

编辑脚本中的 `test_new_features()` 函数：

```bash
test_new_features() {
    # 添加版本专属测试
    echo "🔍 新功能：专家系统..."
    RESULT=$(curl -sf "$BASE_URL/experts" 2>&1)
    if echo "$RESULT" | grep -q '"data"'; then
        record_test "专家系统" "PASS" "功能正常"
    else
        record_test "专家系统" "FAIL" "功能异常"
    fi
}
```

---

## 文档体系

### 版本迭代文档

每个版本在 `docs/releases/` 目录下创建专属文件夹：

```
docs/releases/
├── TEMPLATE_REQUIREMENTS.md      # 需求规格模板
├── TEMPLATE_DESIGN.md            # 概要设计模板
├── TEMPLATE_TEST_CASES.md        # 测试用例模板
├── TEMPLATE_RELEASE_CHECKLIST.md # 发布清单模板
├── VERSION_WORKFLOW.md           # 版本迭代流程说明
│
├── v0.1.0/                       # v0.1.0 版本文档
│   ├── REQUIREMENTS.md           # 需求规格
│   ├── DESIGN.md                 # 概要设计
│   ├── TEST_CASES.md             # 测试用例
│   └── RELEASE_CHECKLIST.md      # 发布清单
│
└── v0.2.0/                       # v0.2.0 版本文档
    └── ...
```

### 文档模板

| 模板 | 用途 | 填写时机 |
|------|------|---------|
| 需求规格说明书 | 定义版本需求和用户故事 | 需求分析阶段 |
| 概要设计文档 | 技术方案和架构设计 | 设计评审阶段 |
| 测试用例文档 | 详细测试用例和验收标准 | 测试准备阶段 |
| 发布清单 | 发布检查项和部署步骤 | 发布准备阶段 |

### 版本测试脚本

| 文件 | 用途 | 维护时机 |
|------|------|---------|
| version-test-template.sh | 版本测试模板 | 创建新版本时复制 |
| version-test-v0.x.0.sh | 特定版本测试 | 版本开发期间维护 |

---

## 工具对比

| 工具 | 用途 | 环境 | 复杂度 |
|------|------|------|--------|
| `make` | 统一入口 | 本地/Docker | ⭐ |
| `dev.sh` | 本地开发 | 本地 | ⭐⭐ |
| `docker.sh` | 容器管理 | Docker | ⭐⭐ |
| `release-test.sh` | 发布验证 | Docker | ⭐⭐⭐ |
| `test_expert.sh` | 专家测试 | 运行中服务 | ⭐⭐ |
| `pre-commit` | 质量门禁 | Git 钩子 | ⭐ |

---

## 🚀 推荐工作流

### 日常开发

```bash
# 1. 启动服务
make serve

# 2. 开发代码...

# 3. 提交代码（自动触发 pre-commit）
git add .
git commit -m "feat: add new feature"
```

### 完整测试

```bash
# 运行所有检查
make check
```

### 版本开发

```bash
# 1. 创建版本文档
cp docs/releases/TEMPLATE_REQUIREMENTS.md docs/releases/v0.x.0/REQUIREMENTS.md
cp docs/releases/TEMPLATE_DESIGN.md docs/releases/v0.x.0/DESIGN.md

# 2. 创建版本测试脚本
cp version-test-template.sh version-test-v0.x.0.sh
chmod +x version-test-v0.x.0.sh

# 3. 开发中快速验证
./version-test-v0.x.0.sh quick

# 4. 编写测试用例
cp docs/releases/TEMPLATE_TEST_CASES.md docs/releases/v0.x.0/TEST_CASES.md
```

### 发版本

```bash
# 1. 运行发布验证
make release-test

# 2. 运行版本测试
./version-test-v0.x.0.sh full

# 3. 查看测试报告
cat RELEASE_TEST_REPORT.md

# 4. 填写发布清单
cp docs/releases/TEMPLATE_RELEASE_CHECKLIST.md docs/releases/v0.x.0/RELEASE_CHECKLIST.md

# 5. 确认通过后发版
git tag v0.x.0
git push origin v0.x.0
```

### Docker 部署

```bash
# 构建并启动
make docker

# 查看状态
./docker.sh status

# 查看日志
./docker.sh logs

# 停止
make docker-stop
```

---

## 📁 相关文件

```
项目根目录/
├── Makefile                  # 统一命令入口
├── dev.sh                    # 本地开发脚本
├── docker.sh                 # Docker 管理脚本
├── release-test.sh           # 发布验证脚本
├── version-test-template.sh  # 版本测试模板
├── version-test-v0.x.0.sh    # 特定版本测试脚本
├── test_expert.sh            # 专家测试脚本
├── test_expert_v2.sh         # 专家 V2 测试脚本
├── scripts/
│   └── pre-commit            # Git 钩子脚本
├── .git/hooks/
│   └── pre-commit            # 已安装的钩子
├── RELEASE_TEST_REPORT.md    # 发布测试报告（生成）
└── docs/
    ├── SCRIPTS_REFERENCE.md  # 脚本工具参考手册
    └── releases/
        ├── TEMPLATE_REQUIREMENTS.md      # 需求规格模板
        ├── TEMPLATE_DESIGN.md            # 概要设计模板
        ├── TEMPLATE_TEST_CASES.md        # 测试用例模板
        ├── TEMPLATE_RELEASE_CHECKLIST.md # 发布清单模板
        ├── VERSION_WORKFLOW.md           # 版本迭代流程
        └── v0.1.0/
            ├── REQUIREMENTS.md           # v0.1.0 需求规格
            └── ...                       # 其他版本文档
```

---

## 🔧 故障排查

### 问题：端口 8080 被占用

```bash
# 查看占用端口的进程
lsof -ti:8080

# 释放端口
lsof -ti:8080 | xargs kill -9
```

### 问题：Docker 容器无法启动

```bash
# 查看容器日志
./docker.sh logs

# 检查数据库连接
docker exec subhuti-app ping host.docker.internal
```

### 问题：pre-commit 检查失败

```bash
# 手动运行检查
cargo fmt --all
cargo clippy --workspace -- -D warnings
cargo test --workspace

# 修复后重新提交
git add .
git commit -m "fix: address review comments"
```

### 问题：发布测试失败

```bash
# 查看详细输出
cat release-test-output.log

# 查看测试报告
cat RELEASE_TEST_REPORT.md

# 单独运行某个阶段
./docker.sh build    # 测试 Docker 构建
./docker.sh start    # 测试容器启动
curl http://localhost:8080/subhuti/api/v1/health  # 测试 API
```

---

## 📝 维护说明

### 添加新脚本

1. 在根目录创建 `.sh` 文件
2. 添加执行权限：`chmod +x script.sh`
3. 在 Makefile 中添加对应 target（如适用）
4. 更新本文档

### 更新 pre-commit

```bash
# 编辑脚本
vim scripts/pre-commit

# 重新安装
make install
```

### 更新发布测试

编辑 `release-test.sh`，添加新的测试项：

```bash
# 在阶段 6 添加新的 API 测试
echo "🔍 新功能测试..."
RESULT=$(curl -sf http://localhost:8080/subhuti/api/v1/new-feature)
if echo "$RESULT" | grep -q '"success"'; then
    record_test "新功能" "PASS" "功能正常"
else
    record_test "新功能" "FAIL" "功能异常"
fi
```

---

**最后更新**: 2026-06-28  
**维护者**: Subhuti Team
