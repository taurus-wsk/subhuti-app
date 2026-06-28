# v0.2.0 概要设计文档

> **版本**: v0.2.0  
> **日期**: 2026-06-28  
> **状态**: 已批准  
> **作者**: Subhuti Team

---

## 设计概述

### 设计目标

增强日志查询 API，在不影响现有功能的前提下，添加过滤、时间范围和分页功能。

### 设计原则

- **向后兼容**: 不改变现有 API 行为
- **性能优先**: 限制单次读取行数
- **参数验证**: 严格验证输入参数

---

## 接口设计

### API: 增强日志查询

**端点**: `GET /subhuti/api/v1/logs`

**查询参数**:

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `level` | String | 否 | 无 | 日志级别：DEBUG/INFO/WARN/ERROR |
| `start` | String | 否 | 无 | 开始时间（ISO 8601） |
| `end` | String | 否 | 无 | 结束时间（ISO 8601） |
| `limit` | u32 | 否 | 50 | 返回条数（最大 200） |
| `offset` | u32 | 否 | 0 | 偏移量 |

**响应格式**:

```json
{
  "success": true,
  "data": {
    "logs": [
      {
        "timestamp": "2026-06-28T10:30:00Z",
        "level": "ERROR",
        "message": "Database connection failed",
        "trace_id": "abc-123"
      }
    ],
    "total": 150,
    "limit": 50,
    "offset": 0
  }
}
```

---

## 模块设计

### 函数签名

```rust
async fn logs_handler(
    Query(params): Query<LogQueryParams>,
) -> impl IntoResponse {
    // 1. 验证参数
    // 2. 读取日志文件
    // 3. 过滤日志
    // 4. 分页处理
    // 5. 返回结果
}

struct LogQueryParams {
    level: Option<String>,
    start: Option<String>,
    end: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
}
```

### 过滤逻辑

```
读取日志文件
  ↓
解析每行 JSON
  ↓
应用 level 过滤（如果有）
  ↓
应用时间范围过滤（如果有）
  ↓
按时间倒序排序
  ↓
应用分页（offset + limit）
  ↓
返回结果
```

---

## 性能设计

### 优化策略

1. **限制读取行数**: 最多读取 1000 行
2. **提前终止**: 找到足够日志后停止
3. **流式处理**: 避免全量加载到内存

### 性能指标

| 操作 | 目标 | 测量方法 |
|------|------|---------|
| 无过滤查询 | < 100ms | API 响应时间 |
| 有过滤查询 | < 200ms | API 响应时间 |

---

## 测试策略

### 单元测试

- 参数验证测试
- 过滤逻辑测试
- 分页逻辑测试

### 集成测试

- API 端到端测试
- 各种参数组合测试

---

## 审批

| 角色 | 姓名 | 签字 | 日期 |
|------|------|------|------|
| 架构师 | | | 2026-06-28 |
| 技术负责人 | | | 2026-06-28 |
