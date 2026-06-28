# Subhuti AI 开发指令卡

> **适用对象**: AI Assistant  
> **用途**: 快速了解标准开发流程  
> **完整手册**: [STANDARD_WORKFLOW.md](./STANDARD_WORKFLOW.md)

---

## 🎯 核心原则

1. **文档先行** - 先写需求/设计文档，再写代码
2. **质量门禁** - 提交前必须通过 `make check`
3. **测试覆盖** - 新功能必须添加测试
4. **可追溯** - 需求→设计→测试→发布完整链路

---

## 📋 五阶段流程

### 阶段 1: 需求分析

```bash
# 1. 创建版本文档
mkdir -p docs/releases/vX.X.0

# 2. 复制模板
cp docs/releases/TEMPLATE_REQUIREMENTS.md docs/releases/vX.X.0/REQUIREMENTS.md

# 3. 填写文档
# - 版本信息（版本号、日期、作者）
# - 版本目标和范围
# - 需求清单（含优先级 P0/P1/P2）
# - 用户故事（至少 1 个）
# - 验收标准
```

**关键点**：
- 需求必须有明确的验收标准
- 优先级必须标注（P0 阻塞发布）
- 用户故事格式：作为XXX，我希望YYY

---

### 阶段 2: 设计评审

```bash
# 1. 复制模板
cp docs/releases/TEMPLATE_DESIGN.md docs/releases/vX.X.0/DESIGN.md

# 2. 填写文档
# - 设计目标和原则
# - 接口设计（API 端点、参数、响应）
# - 模块设计（函数签名、数据结构）
# - 性能设计（如适用）
```

**关键点**：
- 接口定义必须完整（参数、类型、必填、默认值）
- 函数参数 <= 7 个（超过则封装结构体）
- 包含错误码定义

---

### 阶段 3: 开发实现

```bash
# 1. 创建分支
git checkout -b feature/vX.X.0-feature-name

# 2. 编码（遵循设计文档）

# 3. 提交前检查
make check  # fmt + clippy + test

# 4. 提交代码
git add .
git commit -m "feat: implement feature name

- Add API endpoint
- Add unit tests
- Update documentation"
```

**代码规范**：
```rust
// ✅ 正确：参数过多时封装结构体
struct MyParams {
    param1: String,
    param2: String,
}

fn my_function(params: &MyParams) -> Result<T> {
    // 使用 params.param1
}

// ✅ 正确：使用结构体更新语法
let obj = MyStruct {
    field1: value1,
    ..Default::default()
};
```

**常见 Clippy 错误修复**：

| 错误 | 原因 | 解决方案 |
|------|------|---------|
| `too_many_arguments` | 参数 > 7 | 封装为结构体 |
| `field_reassign_with_default` | default 后修改字段 | 使用 `{ field: value, ..Default::default() }` |
| `needless_range_loop` | 使用索引遍历 | 使用 `.iter()` |

---

### 阶段 4: 测试验证

```bash
# 1. 创建版本测试脚本
cp version-test-template.sh version-test-vX.X.0.sh
chmod +x version-test-vX.X.0.sh

# 2. 编辑测试脚本（添加新功能测试）
# 修改 test_new_features() 函数

# 3. 运行版本测试
./scripts/build/dev.sh start
./scripts/test/version-test-vX.X.0.sh quick  # 开发中
./scripts/test/version-test-vX.X.0.sh full   # 发布前
./scripts/build/dev.sh stop

# 4. 运行发布验证
make release-test

# 5. 编写测试用例文档
cp docs/releases/TEMPLATE_TEST_CASES.md docs/releases/vX.X.0/TEST_CASES.md
```

**测试脚本模板**：
```bash
# API 测试
echo "🔍 测试名称..."
RESULT=$(curl -sf "$BASE_URL/path?param=value" 2>&1)
if echo "$RESULT" | grep -q '"expected"'; then
    record_test "测试名称" "PASS" "详细说明"
else
    record_test "测试名称" "FAIL" "错误说明"
fi
```

---

### 阶段 5: 发布部署

```bash
# 1. 填写发布清单
cp docs/releases/TEMPLATE_RELEASE_CHECKLIST.md docs/releases/vX.X.0/RELEASE_CHECKLIST.md

# 2. 更新版本号（Cargo.toml）

# 3. 最终验证
make check
make release-test
./scripts/test/version-test-vX.X.0.sh full

# 4. 提交并打标签
git add .
git commit -m "chore: release vX.X.0"
git tag -a vX.X.0 -m "Release vX.X.0: Description"
git push origin main
git push origin vX.X.0

# 5. 构建 Docker 镜像
docker build -t subhuti:vX.X.0 .
docker tag subhuti:vX.X.0 subhuti:latest

# 6. 部署
./scripts/build/docker.sh stop
docker run -d --name subhuti-app -p 8080:8080 subhuti:vX.X.0

# 7. 验证
curl http://localhost:8080/subhuti/api/v1/health
./scripts/test/version-test-vX.X.0.sh quick
```

---

## 🔧 常用命令速查

### 开发
```bash
make serve              # 启动服务
make check              # 运行检查（fmt + clippy + test）
./scripts/build/dev.sh logs           # 查看日志
./scripts/build/dev.sh restart        # 重启服务
```

### 测试
```bash
make release-test       # 发布验证（14 项测试）
./scripts/test/version-test-vX.X.0.sh quick  # 版本快速测试
./scripts/test/version-test-vX.X.0.sh full   # 版本完整测试
cargo test --workspace          # 单元测试
```

### 部署
```bash
make docker             # Docker 构建+启动
./scripts/build/docker.sh stop        # 停止容器
./scripts/build/docker.sh logs        # 查看日志
./scripts/build/docker.sh status      # 容器状态
```

### Git
```bash
git commit -m "feat: ..."     # 提交（自动触发 pre-commit）
git tag -a vX.X.0 -m "..."    # 打标签
git push origin vX.X.0        # 推送标签
```

---

## 📁 文档结构

```
docs/releases/
├── STANDARD_WORKFLOW.md           # 📋 标准流程手册（完整）
├── README.md                      # 📖 快速指南
├── TEMPLATE_REQUIREMENTS.md       # 📝 需求模板
├── TEMPLATE_DESIGN.md             # 🏗️ 设计模板
├── TEMPLATE_TEST_CASES.md         # 🧪 测试模板
├── TEMPLATE_RELEASE_CHECKLIST.md  # ✅ 发布清单模板
│
└── vX.X.0/                        # 版本文档
    ├── REQUIREMENTS.md
    ├── DESIGN.md
    ├── TEST_CASES.md
    └── RELEASE_CHECKLIST.md
```

---

## ⚠️ 注意事项

### 必须做
- ✅ 先写文档，再写代码
- ✅ 提交前运行 `make check`
- ✅ 新功能添加测试
- ✅ 使用模板创建文档

### 不要做
- ❌ 跳过文档直接写代码
- ❌ 忽略 Clippy 警告
- ❌ 不写测试
- ❌ 使用 `--no-verify` 跳过检查（除非紧急）

---

## 🐛 故障排查

### Clippy 失败
```bash
# 自动修复
cargo clippy --fix --allow-dirty

# 查看问题
cargo clippy --workspace -- -D warnings
```

### 发布验证失败
```bash
# 查看报告
cat RELEASE_TEST_REPORT.md

# 常见原因
cargo fmt --all              # 格式问题
cargo clippy --workspace     # 警告问题
./scripts/build/docker.sh logs             # Docker 问题
```

### 服务启动失败
```bash
# 查看日志
./scripts/build/dev.sh logs

# 释放端口
lsof -ti:8080 | xargs kill -9
```

---

## 📊 检查清单

### 版本启动
- [ ] 创建版本文档目录
- [ ] 复制文档模板
- [ ] 创建版本测试脚本
- [ ] 创建功能分支

### 开发中
- [ ] 遵循设计文档
- [ ] 编写单元测试
- [ ] 运行 `make check`

### 发布前
- [ ] 需求文档已填写
- [ ] 设计文档已填写
- [ ] 测试用例已填写
- [ ] 版本测试通过
- [ ] 发布验证通过（14/14）
- [ ] 发布清单已填写

### 发布后
- [ ] Git 标签已推送
- [ ] Docker 镜像已构建
- [ ] 部署成功
- [ ] 验证通过

---

## 📚 相关文档

- **完整手册**: [STANDARD_WORKFLOW.md](./STANDARD_WORKFLOW.md)
- **脚本参考**: [../SCRIPTS_REFERENCE.md](../SCRIPTS_REFERENCE.md)
- **架构文档**: [../ARCHITECTURE.md](../ARCHITECTURE.md)
- **API 教程**: [../API_TUTORIAL.md](../API_TUTORIAL.md)

---

**版本**: v1.0  
**更新**: 2026-06-28  
**维护**: Subhuti Team
