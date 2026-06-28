# Subhuti 标准开发流程手册

> **版本**: v1.0  
> **更新日期**: 2026-06-28  
> **适用范围**: 所有版本迭代  
> **目标读者**: 开发者、AI Assistant、技术团队

---

## 📋 目录

- [流程概览](#流程概览)
- [阶段 1: 需求分析](#阶段-1-需求分析)
- [阶段 2: 设计评审](#阶段-2-设计评审)
- [阶段 3: 开发实现](#阶段-3-开发实现)
- [阶段 4: 测试验证](#阶段-4-测试验证)
- [阶段 5: 发布部署](#阶段-5-发布部署)
- [快速参考](#快速参考)
- [常见问题](#常见问题)
- [检查清单](#检查清单)

---

## 流程概览

### 五阶段流程

```
需求分析 → 设计评审 → 开发实现 → 测试验证 → 发布部署
   ↓           ↓           ↓           ↓           ↓
需求文档   设计文档    代码实现    测试报告    发布清单
```

### 关键原则

1. **文档先行** - 先写文档，再写代码
2. **质量门禁** - pre-commit 自动检查
3. **自动化测试** - 版本测试 + 发布验证
4. **可追溯** - 需求→设计→测试→发布完整链路

### 适用场景

| 迭代类型 | 流程执行 | 文档要求 |
|---------|---------|---------|
| 大功能（> 3 天） | 完整流程 | 所有文档 |
| 中功能（1-3 天） | 完整流程 | 简化文档 |
| 小功能（< 1 天） | 简化流程 | 需求+测试 |
| Bug 修复 | 简化流程 | 测试用例 |

---

## 阶段 1: 需求分析

### 目标

明确版本要解决的问题和实现的功能。

### 输入

- 用户反馈
- 产品规划
- 技术债务

### 输出

- ✅ `docs/releases/vX.X.0/REQUIREMENTS.md`

### 执行步骤

#### 步骤 1: 创建版本文档目录

```bash
# 替换 vX.X.0 为实际版本号，如 v0.2.0
mkdir -p docs/releases/vX.X.0
```

#### 步骤 2: 复制需求模板

```bash
cp docs/releases/TEMPLATE_REQUIREMENTS.md docs/releases/vX.X.0/REQUIREMENTS.md
```

#### 步骤 3: 填写需求文档

打开 `docs/releases/vX.X.0/REQUIREMENTS.md`，填写以下内容：

1. **版本信息**（顶部元数据）
   ```markdown
   > **版本**: vX.X.0  
   > **日期**: YYYY-MM-DD  
   > **状态**: 草稿/评审中/已批准  
   > **作者**: XXX
   ```

2. **版本概述**
   - 版本目标：一句话描述核心目标
   - 范围：包含和不包含的功能
   - 依赖关系：前置版本和外部依赖

3. **需求清单**（必填）
   ```markdown
   | ID | 需求名称 | 类型 | 优先级 | 状态 | 备注 |
   |----|---------|------|--------|------|------|
   | REQ-001 | 功能名称 | 功能 | P0 | 待开发 | 说明 |
   ```
   
   **优先级定义**：
   - P0: 必须完成（阻塞发布）
   - P1: 应该完成（影响质量）
   - P2: 可以完成（锦上添花）

4. **用户故事**（至少 1 个）
   ```markdown
   ### US-001: 作为XXX，我希望YYY
   
   **场景**：
   1. 步骤 1
   2. 步骤 2
   3. 步骤 3
   
   **验收条件**：
   - [ ] 条件 1
   - [ ] 条件 2
   ```

5. **非功能需求**（如适用）
   - 性能指标
   - 可用性要求
   - 安全要求

6. **验收标准**
   - 功能验收清单
   - 质量验收清单
   - 文档验收清单

### 检查清单

- [ ] 版本信息已更新
- [ ] 版本目标清晰
- [ ] 需求清单完整（含优先级）
- [ ] 至少 1 个用户故事
- [ ] 验收标准明确

### 示例

参考：[v0.2.0/REQUIREMENTS.md](./v0.2.0/REQUIREMENTS.md)

---

## 阶段 2: 设计评审

### 目标

设计技术方案，确保技术可行性和架构合理性。

### 输入

- 需求规格说明书
- 现有架构文档

### 输出

- ✅ `docs/releases/vX.X.0/DESIGN.md`

### 执行步骤

#### 步骤 1: 复制设计模板

```bash
cp docs/releases/TEMPLATE_DESIGN.md docs/releases/vX.X.0/DESIGN.md
```

#### 步骤 2: 填写设计文档

打开 `docs/releases/vX.X.0/DESIGN.md`，填写以下内容：

1. **设计概述**
   - 设计目标
   - 设计原则（2-3 条）
   - 技术约束

2. **架构设计**（如适用）
   - 整体架构图（文本或 Mermaid）
   - 模块关系图
   - 数据流图

3. **接口设计**（必填）
   ```markdown
   ### API: 功能名称
   
   **端点**: `GET/POST /path/to/api`
   
   **请求参数**:
   | 参数 | 类型 | 必填 | 默认值 | 说明 |
   |------|------|------|--------|------|
   | param1 | String | 是 | - | 说明 |
   
   **响应格式**:
   ```json
   {
     "success": true,
     "data": { ... }
   }
   ```
   ```

4. **模块设计**（必填）
   - 函数签名
   - 数据结构
   - 关键算法

5. **性能设计**（如适用）
   - 优化策略
   - 性能指标

6. **测试策略**
   - 单元测试计划
   - 集成测试计划
   - 性能测试计划

### 检查清单

- [ ] 设计目标清晰
- [ ] 接口定义完整
- [ ] 模块设计合理
- [ ] 性能方案可行
- [ ] 测试策略明确

### 示例

参考：[v0.2.0/DESIGN.md](./v0.2.0/DESIGN.md)

---

## 阶段 3: 开发实现

### 目标

按照设计文档实现功能代码。

### 输入

- 概要设计文档
- 接口文档

### 输出

- ✅ 功能代码
- ✅ 单元测试
- ✅ 代码注释

### 执行步骤

#### 步骤 1: 环境准备

```bash
# 安装 pre-commit hook（首次）
make install

# 创建功能分支
git checkout -b feature/vX.X.0-feature-name
```

#### 步骤 2: 编码实现

**编码规范**：

1. **遵循设计文档**
   - 按照接口设计实现 API
   - 按照模块设计实现函数

2. **代码质量**
   - 函数参数 <= 7 个（超过则封装结构体）
   - 函数长度 <= 50 行（超过则拆分）
   - 添加必要的注释

3. **错误处理**
   - 使用 `Result<T, E>` 处理错误
   - 提供有意义的错误信息
   - 不要吞掉错误

**示例**：

```rust
// ❌ 错误：参数过多
fn process_data(
    param1: String,
    param2: String,
    param3: String,
    param4: String,
    param5: String,
    param6: String,
    param7: String,
    param8: String,
) -> Result<()> {
    // ...
}

// ✅ 正确：封装为结构体
struct ProcessParams {
    param1: String,
    param2: String,
    // ...
}

fn process_data(params: &ProcessParams) -> Result<()> {
    // ...
}
```

#### 步骤 3: 编写单元测试

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_name() {
        // 测试正常情况
        assert!(function_name(input).is_ok());
        
        // 测试边界情况
        assert!(function_name(edge_input).is_err());
    }
}
```

#### 步骤 4: 提交前检查

```bash
# 格式化代码
cargo fmt --all

# 运行 clippy
cargo clippy --workspace -- -D warnings

# 运行测试
cargo test --workspace

# 或使用 Makefile 一键检查
make check
```

#### 步骤 5: 提交代码

```bash
git add .
git commit -m "feat: implement feature name

- Add API endpoint
- Add unit tests
- Update documentation"
```

**提交规范**：

- `feat:` 新功能
- `fix:` 修复 bug
- `docs:` 文档更新
- `refactor:` 重构代码
- `test:` 测试相关
- `chore:` 构建/工具链

### 常见问题

#### Q1: Clippy 报错 "too many arguments"

**原因**：函数参数超过 7 个

**解决**：
```rust
// 1. 创建结构体
struct MyParams {
    param1: Type1,
    param2: Type2,
    // ...
}

// 2. 修改函数签名
fn my_function(params: &MyParams) -> Result<T> {
    // 使用 params.param1, params.param2
}
```

#### Q2: Clippy 报错 "field_reassign_with_default"

**原因**：使用 `Default::default()` 后又修改字段

**解决**：
```rust
// ❌ 错误
let mut obj = MyStruct::default();
obj.field1 = value1;
obj.field2 = value2;

// ✅ 正确
let obj = MyStruct {
    field1: value1,
    field2: value2,
    ..Default::default()
};
```

#### Q3: 编译通过但 clippy 失败

**原因**：clippy 有额外的代码质量检查

**解决**：
```bash
# 自动修复（部分问题）
cargo clippy --fix --allow-dirty

# 手动修复剩余问题
cargo clippy --workspace -- -D warnings
```

### 检查清单

- [ ] 代码符合设计文档
- [ ] 单元测试覆盖率 > 80%
- [ ] `make check` 全部通过
- [ ] 代码已格式化
- [ ] 无 Clippy 警告
- [ ] 提交信息规范

---

## 阶段 4: 测试验证

### 目标

验证功能是否符合需求，质量是否达标。

### 输入

- 功能代码
- 测试用例文档

### 输出

- ✅ `docs/releases/vX.X.0/TEST_CASES.md`
- ✅ `version-test-vX.X.0.sh`
- ✅ 测试报告

### 执行步骤

#### 步骤 1: 创建版本测试脚本

```bash
# 从模板复制
cp version-test-template.sh version-test-vX.X.0.sh
chmod +x version-test-vX.X.0.sh
```

#### 步骤 2: 编辑测试脚本

打开 `version-test-vX.X.0.sh`，找到 `test_new_features()` 函数，添加版本专属测试：

```bash
test_new_features() {
    echo ""
    echo -e "${BLUE}新功能测试 - vX.X.0 (功能名称)${NC}"
    echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
    
    # 测试 1: 功能点 1
    echo "🔍 测试功能点 1..."
    RESULT=$(curl -sf "$BASE_URL/api-endpoint" 2>&1)
    if echo "$RESULT" | grep -q '"expected_field"'; then
        record_test "功能点 1" "PASS" "功能正常"
    else
        record_test "功能点 1" "FAIL" "功能异常"
    fi
    
    # 测试 2: 功能点 2
    echo "🔍 测试功能点 2..."
    # ... 添加更多测试
}
```

**测试脚本模板**：

```bash
# API 测试模板
echo "🔍 测试名称..."
RESULT=$(curl -sf "$BASE_URL/path?param=value" 2>&1)
if echo "$RESULT" | grep -q '"expected"'; then
    record_test "测试名称" "PASS" "详细说明"
else
    record_test "测试名称" "FAIL" "错误说明"
fi

# JSON 解析模板
VALUE=$(echo "$RESULT" | python3 -c "import sys,json; print(json.load(sys.stdin).get('key',''))" 2>/dev/null)
if [ "$VALUE" = "expected" ]; then
    record_test "测试名称" "PASS" "值: $VALUE"
else
    record_test "测试名称" "FAIL" "期望 expected, 实际 $VALUE"
fi
```

#### 步骤 3: 编写测试用例文档

```bash
cp docs/releases/TEMPLATE_TEST_CASES.md docs/releases/vX.X.0/TEST_CASES.md
```

填写 `TEST_CASES.md`：

1. **测试概述**
2. **单元测试用例**（UT-001, UT-002...）
3. **集成测试用例**（IT-001, IT-002...）
4. **API 测试用例**（API-001, API-002...）
5. **性能测试用例**（PT-001, PT-002...）

#### 步骤 4: 运行版本测试

```bash
# 启动服务
./scripts/build/dev.sh start

# 快速测试（开发中）
./scripts/test/version-test-vX.X.0.sh quick

# 完整测试（发布前）
./scripts/test/version-test-vX.X.0.sh full

# 停止服务
./scripts/build/dev.sh stop
```

**测试模式**：

| 模式 | 说明 | 使用场景 |
|------|------|---------|
| `quick` | 基础功能 + 新功能 | 日常开发验证 |
| `full` | 完整测试（含 API 回归和性能） | 发布前验证 |
| `api` | 仅 API 回归测试 | API 修改后 |
| `perf` | 仅性能测试 | 性能优化后 |

#### 步骤 5: 运行发布验证

```bash
make release-test
```

**发布验证包含**：
1. 代码质量检查（fmt + clippy）
2. Release 编译
3. 单元测试
4. Docker 构建
5. 容器启动
6. API 功能测试（14 项）
7. 清理

**预期输出**：
```
测试总结
  总计: 14 项
  通过: 14
  失败: 0
  警告: 0
  耗时: 47s

✅ 所有测试通过，可以发布！
📄 测试报告已生成: RELEASE_TEST_REPORT.md
```

#### 步骤 6: 查看测试报告

```bash
cat RELEASE_TEST_REPORT.md
```

### 检查清单

- [ ] 版本测试脚本已创建
- [ ] 新功能测试已添加
- [ ] 版本测试通过（quick 模式）
- [ ] 发布验证通过（14/14）
- [ ] 测试用例文档已填写
- [ ] 测试报告已生成

### 示例

- 版本测试脚本：[version-test-v0.2.0.sh](../../scripts/test/version-test-v0.2.0.sh)
- 测试用例文档：[v0.2.0/TEST_CASES.md](./v0.2.0/TEST_CASES.md)

---

## 阶段 5: 发布部署

### 目标

安全地将新版本部署到生产环境。

### 输入

- 测试通过的代码
- 测试报告

### 输出

- ✅ `docs/releases/vX.X.0/RELEASE_CHECKLIST.md`
- ✅ Git 标签
- ✅ Docker 镜像

### 执行步骤

#### 步骤 1: 填写发布清单

```bash
cp docs/releases/TEMPLATE_RELEASE_CHECKLIST.md docs/releases/vX.X.0/RELEASE_CHECKLIST.md
```

逐项检查并填写 `RELEASE_CHECKLIST.md`：

1. **需求验证** - 所有 P0/P1 需求已完成
2. **代码质量** - make check 全部通过
3. **测试验证** - 版本测试和发布验证通过
4. **文档更新** - 所有文档已完成
5. **配置检查** - 版本号、配置文件已更新
6. **部署准备** - Docker 镜像构建成功

#### 步骤 2: 更新版本号

编辑 `Cargo.toml`：

```toml
[package]
name = "subhuti-app"
version = "X.X.0"  # 更新版本号
```

#### 步骤 3: 最终验证

```bash
# 运行所有检查
make check

# 运行发布验证
make release-test

# 运行版本测试
./scripts/test/version-test-vX.X.0.sh full
```

#### 步骤 4: 提交并打标签

**方式 A：手动操作**

```bash
# 提交所有更改
git add .
git commit -m "chore: release vX.X.0

- Update version number
- Add release documentation
- Update CHANGELOG"

# 打标签（附注标签，推荐）
# 格式：git tag -a <version> -m "<详细说明>"
git tag -a vX.X.0 -m "Release vX.X.0: Feature description

## 新增功能
- 功能 1：描述
- 功能 2：描述

## 修复问题
- 修复 1：描述

## 技术改进
- 改进 1：描述"

# 验证标签
git tag -l                    # 查看本地标签列表
git show vX.X.0               # 查看标签详情

# 推送到远程
git push origin main
git push origin vX.X.0        # 推送标签到 GitHub

# 提示：GitHub 会自动创建 Release 页面
# 访问：https://github.com/<user>/<repo>/releases/tag/vX.X.0
```

**方式 B：自动发布（推荐）**

```bash
# 使用自动发布脚本
# 格式：./scripts/release/auto-release.sh <version> [message]
./scripts/release/auto-release.sh vX.X.0 "功能描述"

# 示例：
./scripts/release/auto-release.sh v0.2.0 "日志查询 API 增强"

# 自动完成：
# ✅ 运行发布验证（release-test.sh）
# ✅ 运行版本测试（version-test-enhanced.sh）
# ✅ 提交代码
# ✅ 创建标签
# ✅ 推送到远程 ← 最后一步
#    ↓
# 线上 CI/CD 自动触发构建和部署
```

**标签命名规范**：
- 格式：`vMAJOR.MINOR.PATCH`（语义化版本）
- 示例：`v0.1.0`, `v0.2.0`, `v1.0.0`
- MAJOR：不兼容的 API 修改
- MINOR：向下兼容的功能新增
- PATCH：向下兼容的问题修正

**标签说明编写建议**：
- 第一行：简短描述（50 字符内）
- 空一行
- 后续：详细变更列表（分类列出）
- 使用 Markdown 格式（GitHub 会渲染）

#### 步骤 5: 构建 Docker 镜像

```bash
# 获取版本号（从 Git 标签）
VERSION=$(git describe --tags --exact-match 2>/dev/null || git describe --tags --abbrev=0 2>/dev/null || echo "dev")
echo "📦 构建版本: $VERSION"

# 构建镜像（使用版本号）
docker build -t subhuti:$VERSION .

# 标记 latest（仅正式版本）
if [[ $VERSION == v* ]]; then
    docker tag subhuti:$VERSION subhuti:latest
    echo "✅ 已标记 latest"
fi

# 验证镜像
docker images subhuti

# 可选：推送到 Docker Hub
# docker tag subhuti:$VERSION your-username/subhuti:$VERSION
# docker push your-username/subhuti:$VERSION
```

**Docker 镜像标签规范**：
- 正式版本：`subhuti:v0.2.0`
- 最新版本：`subhuti:latest`
- 开发版本：`subhuti:dev`

#### 步骤 6: 部署到生产环境

```bash
# 停止旧容器
./scripts/build/docker.sh stop

# 启动新容器
docker run -d \
    --name subhuti-app \
    -p 8080:8080 \
    --env-file .env \
    subhuti:vX.X.0

# 验证部署
curl http://localhost:8080/subhuti/api/v1/health

# 运行快速测试
./scripts/test/version-test-vX.X.0.sh quick
```

#### 步骤 7: 监控和回滚

**监控**：
```bash
# 查看容器状态
./scripts/build/docker.sh status

# 查看日志
./scripts/build/docker.sh logs

# 查看健康检查
curl http://localhost:8080/subhuti/api/v1/health/detailed
```

**回滚**（如需要）：
```bash
# 停止当前版本
docker stop subhuti-app
docker rm subhuti-app

# 启动上一版本
docker run -d --name subhuti-app -p 8080:8080 subhuti:vX-1.X.0
```

### 检查清单

- [ ] 发布清单已填写
- [ ] 版本号已更新
- [ ] 最终验证通过
- [ ] 代码已提交
- [ ] Git 标签已推送
- [ ] Docker 镜像已构建
- [ ] 部署成功
- [ ] 验证通过
- [ ] 回滚方案就绪

### 示例

参考：[TEMPLATE_RELEASE_CHECKLIST.md](./TEMPLATE_RELEASE_CHECKLIST.md)

---

## 快速参考

### 常用命令

```bash
# 开发
make serve              # 启动服务
make check              # 运行检查
./scripts/build/dev.sh logs           # 查看日志

# 测试
make release-test       # 发布验证
./scripts/test/version-test-vX.X.0.sh quick  # 版本快速测试
./scripts/test/version-test-vX.X.0.sh full   # 版本完整测试

# 部署
make docker             # Docker 部署
./scripts/build/docker.sh status      # 容器状态
./scripts/build/docker.sh logs        # 容器日志

# Git
git commit -m "feat: ..."     # 提交代码
git tag -a vX.X.0 -m "..."    # 打标签
git push origin vX.X.0        # 推送标签
```

### 文档模板路径

```
docs/releases/
├── TEMPLATE_REQUIREMENTS.md      # 需求规格模板
├── TEMPLATE_DESIGN.md            # 概要设计模板
├── TEMPLATE_TEST_CASES.md        # 测试用例模板
├── TEMPLATE_RELEASE_CHECKLIST.md # 发布清单模板
└── VERSION_WORKFLOW.md           # 版本迭代流程
```

### 版本测试脚本

```bash
# 创建
cp version-test-template.sh version-test-vX.X.0.sh
chmod +x version-test-vX.X.0.sh

# 编辑
vim version-test-vX.X.0.sh  # 修改 test_new_features()

# 运行
./scripts/test/version-test-vX.X.0.sh quick
./scripts/test/version-test-vX.X.0.sh full
```

---

## 常见问题

### Q1: 什么时候需要创建新版本？

**A**: 
- 有新功能需要发布
- 有重要 bug 修复
- 有性能优化完成
- 需要给用户明确的版本标识

### Q2: 版本文档必须全部填写吗？

**A**: 
- **大型迭代**（> 3 天）：必须完整填写
- **中型迭代**（1-3 天）：可以简化设计文档
- **小型迭代**（< 1 天）：至少需求和测试
- **Bug 修复**：至少测试用例

### Q3: 版本测试脚本和 release-test.sh 有什么区别？

**A**:
- **release-test.sh**: 通用发布验证，所有版本共用，14 项固定测试
- **version-test-vX.X.0.sh**: 版本专属测试，针对特定需求，自定义测试项

### Q4: 如何管理多个版本的文档？

**A**: 
```
docs/releases/
├── v0.1.0/  # 初始版本
├── v0.2.0/  # 下一个版本
└── v0.3.0/  # 未来版本
```
每个版本独立文件夹，互不干扰。

### Q5: Clippy 检查失败怎么办？

**A**:
```bash
# 1. 自动修复（部分问题）
cargo clippy --fix --allow-dirty

# 2. 查看具体问题
cargo clippy --workspace -- -D warnings

# 3. 手动修复后重新检查
cargo fmt --all
cargo clippy --workspace -- -D warnings
```

### Q6: 发布验证失败怎么办？

**A**:
1. 查看测试报告：`cat RELEASE_TEST_REPORT.md`
2. 查看失败项，定位问题
3. 修复后重新运行：`make release-test`
4. 常见失败原因：
   - 代码格式问题 → `cargo fmt --all`
   - Clippy 警告 → 修复警告
   - Docker 构建失败 → 检查 Dockerfile
   - API 测试失败 → 检查服务是否启动

### Q7: 如何跳过 pre-commit 检查？

**A**（不推荐）：
```bash
# 仅用于紧急修复
git commit --no-verify -m "hotfix: urgent fix"
```

---

## 检查清单

### 版本启动检查

- [ ] 创建版本文档目录
- [ ] 复制文档模板
- [ ] 创建版本测试脚本
- [ ] 创建功能分支

### 开发中检查

- [ ] 遵循设计文档
- [ ] 编写单元测试
- [ ] 及时提交代码
- [ ] 运行 `make check`

### 发布前检查

- [ ] 需求文档已填写
- [ ] 设计文档已填写
- [ ] 测试用例已填写
- [ ] 版本测试通过
- [ ] 发布验证通过
- [ ] 发布清单已填写
- [ ] 版本号已更新
- [ ] 代码已提交

### 发布后检查

- [ ] Git 标签已推送
- [ ] Docker 镜像已构建
- [ ] 部署成功
- [ ] 验证通过
- [ ] 文档归档

---

## 附录

### 提交信息规范

```
<type>: <subject>

<body>

<footer>
```

**type**:
- `feat`: 新功能
- `fix`: 修复 bug
- `docs`: 文档更新
- `style`: 代码格式
- `refactor`: 重构
- `test`: 测试
- `chore`: 构建/工具

**示例**：
```
feat: add log query time range filter

- Add start/end time parameters to logs API
- Implement time range filtering logic
- Add unit tests
- Update API documentation

Closes #123
```

### 版本号规范

遵循 [Semantic Versioning](https://semver.org/)：

- **MAJOR** (X.0.0): 不兼容的 API 变更
- **MINOR** (0.X.0): 向后兼容的功能
- **PATCH** (0.0.X): 向后兼容的 bug 修复

**示例**：
- v0.1.0 → v0.2.0: 新增功能
- v0.2.0 → v0.2.1: bug 修复
- v0.2.0 → v1.0.0: 重大变更

### 相关文档

- [脚本工具参考手册](../SCRIPTS_REFERENCE.md)
- [架构文档](../ARCHITECTURE.md)
- [API 教程](../API_TUTORIAL.md)
- [用户指南](../USER_GUIDE.md)

---

**文档版本**: v1.0  
**最后更新**: 2026-06-28  
**维护者**: Subhuti Team  
**反馈渠道**: 提交 Issue 或联系维护者
