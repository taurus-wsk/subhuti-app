#!/bin/bash
# ============================================================
# 清理 .DS_Store 文件
# ============================================================
# 用途：清理项目中所有的 .DS_Store 文件
# 用法：./scripts/cleanup-dsstore.sh
# ============================================================
set -e

echo "🧹 清理 .DS_Store 文件..."

# 从 Git 中移除
echo "📤 从 Git 缓存中移除..."
find . -name ".DS_Store" -type f -exec git rm --cached {} \; 2>/dev/null || true

# 删除本地文件
echo "🗑️  删除本地文件..."
find . -name ".DS_Store" -type f -delete

echo "✅ 清理完成！"
echo ""
echo "💡 提示：已配置 .gitignore，以后不会再产生 .DS_Store 文件"
