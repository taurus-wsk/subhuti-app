# Subhuti 版本迭代指南

> 本文档指导团队如何按照工程化流程完成版本迭代。

---

## 🚨 重要：标准流程手册

**所有开发人员和 AI Assistant 请首先阅读**：
👉 [**STANDARD_WORKFLOW.md**](./STANDARD_WORKFLOW.md) - 完整标准流程手册

该手册包含：
- ✅ 详细的五阶段流程说明
- ✅ 每个步骤的具体命令
- ✅ 代码示例和最佳实践
- ✅ 常见问题和解决方案
- ✅ 检查清单

**本文档**提供快速上手指引，详细流程请参考标准手册。

---

## 📋 目录

- [快速开始](#快速开始)
- [版本迭代流程](#版本迭代流程)
- [文档模板使用](#文档模板使用)
- [版本测试脚本](#版本测试脚本)
- [发布流程](#发布流程)
- [常见问题](#常见问题)

---

## 快速开始

### 新项目启动

```bash
# 1. 克隆仓库
git clone <repo-url>
cd subhuti-app

# 2. 安装工具
make install  # 安装 pre-commit hook

# 3. 启动服务
make serve

# 4. 验证环境
make check
```

### 创建新版本

```bash
# 1. 创建版本文档目录
mkdir -p docs/releases/v0.x.0

# 2. 复制文档模板
cp docs/releases/TEMPLATE_REQUIREMENTS.md docs/releases/v0.x.0/REQUIREMENTS.md
cp docs/releases/TEMPLATE_DESIGN.md docs/releases/v0.x.0/DESIGN.md
cp docs/releases/TEMPLATE_TEST_CASES.md docs/releases/v0.x.0/TEST_CASES.md
cp docs/releases/TEMPLATE_RELEASE_CHECKLIST.md docs/releases/v0.x.0/RELEASE_CHECKLIST.md

# 3. 创建版本测试脚本
cp version-test-template.sh version-test-v0.x.0.sh
chmod +x version-test-v0.x.0.sh
```

---

## 版本迭代流程

完整流程详见：[VERSION_WORKFLOW.md](./VERSION_WORKFLOW.md)

### 五个阶段

1. **需求分析** → 填写需求规格说明书
2. **设计评审** → 编写概要设计文档
3. **开发实现** → 编码 + 单元测试
4. **测试验证** → 执行版本测试 + 发布验证
5. **发布部署** → 填写发布清单 + 部署

### 关键产出

| 阶段 | 文档 | 脚本 |
|------|------|------|
| 需求分析 | REQUIREMENTS.md | - |
| 设计评审 | DESIGN.md | - |
| 开发实现 | - | 代码 + 单元测试 |
| 测试验证 | TEST_CASES.md | version-test-v0.x.0.sh |
| 发布部署 | RELEASE_CHECKLIST.md | release-test.sh |

---

## 文档模板使用

### 需求规格说明书

**文件**：`docs/releases/v0.x.0/REQUIREMENTS.md`

**填写步骤**：

1. 更新版本信息（顶部元数据）
2. 编写版本目标和范围
3. 填写需求清单（含优先级）
4. 编写用户故事
5. 定义非功能需求
6. 设置验收标准

**示例**：查看 [v0.1.0/REQUIREMENTS.md](./v0.1.0/REQUIREMENTS.md)

---

### 概要设计文档

**文件**：`docs/releases/v0.x.0/DESIGN.md`

**填写步骤**：

1. 描述设计目标和原则
2. 绘制架构图（使用 Mermaid）
3. 设计模块接口
4. 设计数据库表结构
5. 制定性能优化方案
6. 编写测试策略

---

### 测试用例文档

**文件**：`docs/releases/v0.x.0/TEST_CASES.md`

**填写步骤**：

1. 编写单元测试用例（UT-001, UT-002...）
2. 编写集成测试用例（IT-001, IT-002...）
3. 编写 API 测试用例（API-001, API-002...）
4. 编写性能测试用例（PT-001, PT-002...）
5. 记录测试执行结果
6. 记录发现的缺陷

---

### 发布清单

**文件**：`docs/releases/v0.x.0/RELEASE_CHECKLIST.md`

**填写步骤**：

1. 逐项检查发布条件
2. 记录版本信息（新功能、修复、已知问题）
3. 填写质量指标
4. 编写发布步骤
5. 准备回滚方案
6. 编写变更日志

---

## 版本测试脚本

### 创建脚本

```bash
# 从模板复制
cp version-test-template.sh version-test-v0.x.0.sh
chmod +x version-test-v0.x.0.sh
```

### 编辑测试内容

打开脚本，找到 `test_new_features()` 函数，添加版本专属测试：

```bash
test_new_features() {
    echo -e "${BLUE}新功能测试 - v0.x.0${NC}"
    
    # 测试 1: 专家系统
    echo "🧑‍⚕️ 专家列表..."
    EXPERTS=$(curl -sf "$BASE_URL/experts" 2>&1)
    if echo "$EXPERTS" | grep -q '"data"'; then
        record_test "专家系统" "PASS" "API 正常"
    else
        record_test "专家系统" "FAIL" "API 异常"
    fi
    
    # 测试 2: 添加更多...
}
```

### 运行测试

```bash
# 快速测试（开发中）
./scripts/test/version-test-v0.x.0.sh quick

# 完整测试（发布前）
./scripts/test/version-test-v0.x.0.sh full

# API 回归测试
./scripts/test/version-test-v0.x.0.sh api

# 性能测试
./scripts/test/version-test-v0.x.0.sh perf
```

---

## 发布流程

### 发布前检查

```bash
# 1. 运行所有代码检查
make check

# 2. 运行发布验证
make release-test

# 3. 运行版本测试
./scripts/test/version-test-v0.x.0.sh full

# 4. 查看测试报告
cat RELEASE_TEST_REPORT.md
```

### 执行发布

```bash
# 1. 填写发布清单
# 编辑 docs/releases/v0.x.0/RELEASE_CHECKLIST.md

# 2. 打标签
git tag -a v0.x.0 -m "Release v0.x.0"
git push origin v0.x.0

# 3. 构建 Docker 镜像
docker build -t subhuti:v0.x.0 .
docker tag subhuti:v0.x.0 subhuti:latest

# 4. 部署
./scripts/build/docker.sh stop
docker run -d --name subhuti-app -p 8080:8080 subhuti:v0.x.0

# 5. 验证部署
curl http://localhost:8080/subhuti/api/v1/health
./scripts/test/version-test-v0.x.0.sh quick
```

---

## 常见问题

### Q1: 什么时候需要创建新版本？

**A**: 当满足以下条件时：
- 有新功能需要发布
- 有重要 bug 修复
- 有性能优化完成
- 需要给用户明确的版本标识

### Q2: 版本文档必须全部填写吗？

**A**: 
- **小型迭代**（bug 修复、小优化）：可以简化文档
- **大型迭代**（新功能、架构变更）：必须完整填写
- 至少要有：需求清单 + 测试用例 + 发布清单

### Q3: 版本测试脚本和 release-test.sh 有什么区别？

**A**:
- **release-test.sh**: 通用发布验证，所有版本共用
- **version-test-v0.x.0.sh**: 版本专属测试，针对特定需求

### Q4: 如何管理多个版本的文档？

**A**: 
```
docs/releases/
├── v0.1.0/  # 初始版本
├── v0.2.0/  # 下一个版本
└── v0.3.0/  # 未来版本
```
每个版本独立文件夹，互不干扰。

### Q5: 版本测试脚本如何维护？

**A**:
- **开发期间**：持续添加新功能的测试用例
- **发布后**：保留脚本作为该版本的验证标准
- **下一版本**：复制模板创建新脚本

### Q6: 文档模板可以修改吗？

**A**: 可以！模板是参考，根据项目实际情况调整：
- 添加项目专属章节
- 删除不需要的部分
- 调整格式和样式

---

## 工具速查

### 常用命令

```bash
# 开发
make serve              # 启动服务
make check              # 运行检查

# 测试
make release-test       # 发布验证
./scripts/test/version-test-v0.x.0.sh quick  # 版本快速测试

# 部署
make docker             # Docker 部署
./scripts/build/docker.sh logs        # 查看日志
```

### 文档位置

```
docs/releases/
├── TEMPLATE_*.md       # 模板文件
├── VERSION_WORKFLOW.md # 流程说明
└── v0.x.0/             # 版本文档
    ├── REQUIREMENTS.md
    ├── DESIGN.md
    ├── TEST_CASES.md
    └── RELEASE_CHECKLIST.md
```

---

## 附录

### 文档检查清单

发布前确保：

- [ ] 需求规格说明书已填写
- [ ] 概要设计文档已填写
- [ ] 测试用例文档已填写
- [ ] 发布清单已完成
- [ ] 版本测试脚本已执行
- [ ] 发布验证已通过

### 相关文档

- [脚本工具参考手册](../SCRIPTS_REFERENCE.md)
- [版本迭代流程](./VERSION_WORKFLOW.md)
- [架构文档](../ARCHITECTURE.md)
- [API 教程](../API_TUTORIAL.md)
- [用户指南](../USER_GUIDE.md)

---

**最后更新**: 2026-06-28  
**维护者**: Subhuti Team
