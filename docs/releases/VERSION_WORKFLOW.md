# Subhuti 版本迭代流程

> 本文档定义了 Subhuti 项目从需求到发布的完整工程化流程。

---

## 📋 目录

- [流程概览](#流程概览)
- [阶段 1: 需求分析](#阶段-1-需求分析)
- [阶段 2: 设计评审](#阶段-2-设计评审)
- [阶段 3: 开发实现](#阶段-3-开发实现)
- [阶段 4: 测试验证](#阶段-4-测试验证)
- [阶段 5: 发布部署](#阶段-5-发布部署)
- [文档模板](#文档模板)
- [工具链](#工具链)

---

## 流程概览

```
需求分析 → 设计评审 → 开发实现 → 测试验证 → 发布部署
   ↓           ↓           ↓           ↓           ↓
需求文档   设计文档    代码+注释    测试报告    发布清单
```

### 关键里程碑

| 阶段 | 产出 | 负责人 | 时长 |
|------|------|--------|------|
| 需求分析 | 需求规格说明书 | 产品经理 | 2-3 天 |
| 设计评审 | 概要设计文档 | 架构师 | 1-2 天 |
| 开发实现 | 代码实现 | 开发团队 | 1-2 周 |
| 测试验证 | 测试报告 | 测试团队 | 2-3 天 |
| 发布部署 | 发布清单 | 运维团队 | 1 天 |

---

## 阶段 1: 需求分析

### 目标

明确版本要解决的问题和实现的功能。

### 输入

- 用户反馈
- 产品规划
- 技术债务

### 活动

1. **需求收集**
   - 收集用户反馈
   - 分析使用数据
   - 评估技术债务

2. **需求分析**
   - 编写需求规格说明书
   - 定义优先级（P0/P1/P2）
   - 编写用户故事

3. **需求评审**
   - 产品评审
   - 技术可行性评估
   - 风险评估

### 产出

- ✅ [需求规格说明书](./TEMPLATE_REQUIREMENTS.md)
- ✅ 需求清单（含优先级）
- ✅ 用户故事

### 检查清单

- [ ] 需求明确且可验证
- [ ] 优先级已定义
- [ ] 风险评估完成
- [ ] 依赖关系清晰
- [ ] 验收标准明确

---

## 阶段 2: 设计评审

### 目标

设计技术方案，确保技术可行性和架构合理性。

### 输入

- 需求规格说明书
- 现有架构文档

### 活动

1. **架构设计**
   - 模块划分
   - 接口设计
   - 数据流设计

2. **详细设计**
   - 数据结构设计
   - 算法设计
   - 性能优化方案

3. **设计评审**
   - 技术评审会议
   - 架构师审批
   - 安全评审

### 产出

- ✅ [概要设计文档](./TEMPLATE_DESIGN.md)
- ✅ 接口文档
- ✅ 数据库设计

### 检查清单

- [ ] 架构设计合理
- [ ] 接口定义清晰
- [ ] 性能方案可行
- [ ] 安全方案完善
- [ ] 向后兼容

---

## 阶段 3: 开发实现

### 目标

按照设计文档实现功能代码。

### 输入

- 概要设计文档
- 接口文档

### 活动

1. **环境准备**
   ```bash
   # 安装 pre-commit hook
   make install
   
   # 创建功能分支
   git checkout -b feature/v0.x.0
   ```

2. **编码实现**
   - 遵循代码规范
   - 编写单元测试
   - 及时提交代码

3. **代码审查**
   ```bash
   # 提交前检查
   make check
   
   # 提交代码
   git commit -m "feat: implement expert system"
   ```

### 产出

- ✅ 功能代码
- ✅ 单元测试
- ✅ 代码注释

### 检查清单

- [ ] 代码符合规范
- [ ] 单元测试覆盖
- [ ] 代码审查通过
- [ ] 文档同步更新

---

## 阶段 4: 测试验证

### 目标

验证功能是否符合需求，质量是否达标。

### 输入

- 功能代码
- 测试用例文档

### 活动

1. **单元测试**
   ```bash
   cargo test --workspace
   ```

2. **集成测试**
   ```bash
   # 启动服务
   ./dev.sh start
   
   # 运行版本测试
   ./version-test-v0.x.0.sh full
   ```

3. **发布验证**
   ```bash
   make release-test
   ```

4. **性能测试**
   ```bash
   wrk -t4 -c100 -d30s http://localhost:8080/subhuti/api/v1/health
   ```

### 产出

- ✅ [测试用例文档](./TEMPLATE_TEST_CASES.md)
- ✅ 测试报告
- ✅ 缺陷记录

### 检查清单

- [ ] 单元测试覆盖率 > 80%
- [ ] 集成测试全部通过
- [ ] 发布验证通过
- [ ] 性能指标达标
- [ ] 缺陷已修复

---

## 阶段 5: 发布部署

### 目标

安全地将新版本部署到生产环境。

### 输入

- 测试通过的代码
- 发布清单

### 活动

1. **发布准备**
   ```bash
   # 更新版本号
   # Cargo.toml: version = "0.x.0"
   
   # 最终验证
   make release-test
   ```

2. **打标签**
   ```bash
   git tag -a v0.x.0 -m "Release v0.x.0"
   git push origin v0.x.0
   ```

3. **构建镜像**
   ```bash
   docker build -t subhuti:v0.x.0 .
   docker tag subhuti:v0.x.0 subhuti:latest
   ```

4. **部署**
   ```bash
   ./docker.sh stop
   docker run -d --name subhuti-app -p 8080:8080 subhuti:v0.x.0
   ```

5. **验证**
   ```bash
   curl http://localhost:8080/subhuti/api/v1/health
   ./version-test-v0.x.0.sh quick
   ```

### 产出

- ✅ [发布清单](./TEMPLATE_RELEASE_CHECKLIST.md)
- ✅ Docker 镜像
- ✅ 发布记录

### 检查清单

- [ ] 发布清单全部勾选
- [ ] 标签已推送
- [ ] 镜像已构建
- [ ] 部署成功
- [ ] 验证通过
- [ ] 回滚方案就绪

---

## 文档模板

### 需求文档

- **模板**: [TEMPLATE_REQUIREMENTS.md](./TEMPLATE_REQUIREMENTS.md)
- **命名**: `v0.x.0_REQUIREMENTS.md`
- **存放**: `docs/releases/`

### 设计文档

- **模板**: [TEMPLATE_DESIGN.md](./TEMPLATE_DESIGN.md)
- **命名**: `v0.x.0_DESIGN.md`
- **存放**: `docs/releases/`

### 测试文档

- **模板**: [TEMPLATE_TEST_CASES.md](./TEMPLATE_TEST_CASES.md)
- **命名**: `v0.x.0_TEST_CASES.md`
- **存放**: `docs/releases/`

### 发布清单

- **模板**: [TEMPLATE_RELEASE_CHECKLIST.md](./TEMPLATE_RELEASE_CHECKLIST.md)
- **命名**: `v0.x.0_RELEASE_CHECKLIST.md`
- **存放**: `docs/releases/`

---

## 工具链

### 开发工具

| 工具 | 用途 | 命令 |
|------|------|------|
| Makefile | 统一入口 | `make help` |
| dev.sh | 本地开发 | `./dev.sh start` |
| pre-commit | 质量门禁 | 自动触发 |

### 测试工具

| 工具 | 用途 | 命令 |
|------|------|------|
| cargo test | 单元测试 | `cargo test --workspace` |
| release-test.sh | 发布验证 | `make release-test` |
| version-test.sh | 版本测试 | `./version-test-v0.x.0.sh` |

### 部署工具

| 工具 | 用途 | 命令 |
|------|------|------|
| docker.sh | 容器管理 | `./docker.sh start` |
| Dockerfile | 镜像构建 | `docker build -t subhuti .` |

---

## 📁 文档结构

```
docs/
├── releases/                    # 版本迭代文档
│   ├── TEMPLATE_REQUIREMENTS.md # 需求规格模板
│   ├── TEMPLATE_DESIGN.md       # 概要设计模板
│   ├── TEMPLATE_TEST_CASES.md   # 测试用例模板
│   ├── TEMPLATE_RELEASE_CHECKLIST.md # 发布清单模板
│   │
│   ├── v0.1.0_REQUIREMENTS.md   # v0.1.0 需求文档
│   ├── v0.1.0_DESIGN.md         # v0.1.0 设计文档
│   ├── v0.1.0_TEST_CASES.md     # v0.1.0 测试文档
│   └── v0.1.0_RELEASE_CHECKLIST.md   # v0.1.0 发布清单
│
├── SCRIPTS_REFERENCE.md         # 脚本工具参考手册
├── ARCHITECTURE.md              # 架构文档
├── API_TUTORIAL.md              # API 教程
└── USER_GUIDE.md                # 用户指南
```

---

## 🔄 持续改进

### 版本回顾

每个版本发布后，进行回顾总结：

1. **做得好的**
   - 继续保持的做法

2. **需要改进的**
   - 遇到的问题
   - 改进建议

3. **行动计划**
   - 具体改进措施
   - 负责人和截止日期

### 流程优化

定期审视流程，持续优化：

- 减少不必要的文档
- 自动化重复性工作
- 提高工具效率

---

## 📊 质量门禁

### 代码提交

```
pre-commit hook
├── cargo fmt --check ✅
├── cargo clippy ✅
└── cargo test ✅
```

### 合并到主分支

```
Pull Request
├── 代码审查通过 ✅
├── CI 全部通过 ✅
└── 测试覆盖率达标 ✅
```

### 发布

```
发布验证
├── make release-test ✅
├── 版本测试脚本 ✅
├── 发布清单完成 ✅
└── 文档已更新 ✅
```

---

## 附录

### 常用命令速查

```bash
# 启动开发环境
make serve

# 运行所有检查
make check

# 发布前验证
make release-test

# 版本测试
./version-test-v0.x.0.sh full

# 部署
make docker
```

### 文档检查清单

每个版本发布前，确保以下文档已完成：

- [ ] 需求规格说明书
- [ ] 概要设计文档
- [ ] 测试用例文档
- [ ] 发布清单
- [ ] 测试报告
- [ ] 变更日志

---

**最后更新**: 2026-06-28  
**维护者**: Subhuti Team
