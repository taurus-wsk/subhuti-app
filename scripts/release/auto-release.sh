#!/bin/bash
# ============================================================
# Subhuti 自动发布脚本
# ============================================================
# 用途：测试通过后自动提交代码、打标签、推送到远程
# 用法：./scripts/release/auto-release.sh [version] [message]
# 示例：
#   ./scripts/release/auto-release.sh v0.2.0 "日志查询 API 增强"
#   ./scripts/release/auto-release.sh v0.3.0 "新增专家系统"
# ============================================================
set -e

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

# 项目根目录
PROJECT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$PROJECT_DIR"

# ============================================================
# 参数解析
# ============================================================
VERSION="${1:-}"
MESSAGE="${2:-}"

if [ -z "$VERSION" ]; then
    echo -e "${RED}❌ 错误：缺少版本号参数${NC}"
    echo ""
    echo "用法: $0 <version> [message]"
    echo ""
    echo "示例:"
    echo "  $0 v0.2.0 \"日志查询 API 增强\""
    echo "  $0 v0.3.0 \"新增专家系统\""
    echo ""
    exit 1
fi

# 验证版本号格式
if [[ ! $VERSION =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo -e "${RED}❌ 错误：版本号格式不正确${NC}"
    echo "期望格式: vX.X.X (例如: v0.2.0)"
    echo "当前输入: $VERSION"
    exit 1
fi

# 如果没有提供说明，使用默认说明
if [ -z "$MESSAGE" ]; then
    MESSAGE="Release $VERSION"
fi

# ============================================================
# 前置检查
# ============================================================
echo -e "${CYAN}╔═══════════════════════════════════════════════════════════╗${NC}"
echo -e "${CYAN}║         Subhuti 自动发布 - $VERSION${NC}"
echo -e "${CYAN}╚═══════════════════════════════════════════════════════════╝${NC}"
echo ""

echo -e "${BLUE}📋 前置检查...${NC}"

# 检查 Git 状态
echo "  检查 Git 状态..."
if [ -n "$(git status --porcelain)" ]; then
    echo -e "  ${YELLOW}⚠️  有未提交的更改${NC}"
    echo "  是否自动提交？(y/n)"
    read -r answer
    if [ "$answer" != "y" ]; then
        echo -e "${RED}❌ 发布取消${NC}"
        exit 1
    fi
    AUTO_COMMIT=true
else
    AUTO_COMMIT=false
fi

# 检查远程连接
echo "  检查远程仓库连接..."
if ! git ls-remote --heads origin &>/dev/null; then
    echo -e "${RED}❌ 无法连接远程仓库${NC}"
    echo "  请检查网络连接或 Git 配置"
    exit 1
fi
echo -e "  ${GREEN}✅ 远程仓库连接正常${NC}"

# 检查标签是否已存在
echo "  检查标签是否存在..."
if git tag -l | grep -q "^${VERSION}$"; then
    echo -e "${RED}❌ 标签 $VERSION 已存在${NC}"
    echo "  请先删除旧标签或选择新版本号"
    exit 1
fi
echo -e "  ${GREEN}✅ 标签可用${NC}"

echo ""

# ============================================================
# 步骤 1: 运行发布验证
# ============================================================
echo -e "${CYAN}═══════════════════════════════════════════════════════════${NC}"
echo -e "${CYAN}📦 步骤 1: 运行发布验证${NC}"
echo -e "${CYAN}═══════════════════════════════════════════════════════════${NC}"
echo ""

if [ -f "scripts/release/release-test.sh" ]; then
    echo -e "${BLUE}🔍 运行 release-test.sh...${NC}"
    if ./scripts/release/release-test.sh; then
        echo -e "${GREEN}✅ 发布验证通过${NC}"
    else
        echo -e "${RED}❌ 发布验证失败${NC}"
        echo "  请修复问题后重试"
        exit 1
    fi
else
    echo -e "${YELLOW}⚠️  release-test.sh 不存在，跳过${NC}"
fi

echo ""

# ============================================================
# 步骤 2: 运行版本测试
# ============================================================
echo -e "${CYAN}═══════════════════════════════════════════════════════════${NC}"
echo -e "${CYAN}📦 步骤 2: 运行版本测试${NC}"
echo -e "${CYAN}═══════════════════════════════════════════════════════════${NC}"
echo ""

# 尝试使用增强版测试脚本
if [ -f "scripts/test/version-test-enhanced.sh" ]; then
    echo -e "${BLUE}🧪 运行版本测试（增强版）...${NC}"
    echo "  测试模式: quick"
    if ./scripts/test/version-test-enhanced.sh quick; then
        echo -e "${GREEN}✅ 版本测试通过${NC}"
    else
        echo -e "${YELLOW}⚠️  版本测试有失败，是否继续？(y/n)${NC}"
        read -r answer
        if [ "$answer" != "y" ]; then
            echo -e "${RED}❌ 发布取消${NC}"
            exit 1
        fi
    fi
elif [ -f "scripts/test/version-test-${VERSION#v}.sh" ]; then
    echo -e "${BLUE}🧪 运行版本测试（${VERSION}）...${NC}"
    VERSION_SCRIPT="scripts/test/version-test-${VERSION#v}.sh"
    if [ -f "$VERSION_SCRIPT" ]; then
        if ./$VERSION_SCRIPT quick; then
            echo -e "${GREEN}✅ 版本测试通过${NC}"
        else
            echo -e "${YELLOW}⚠️  版本测试有失败，是否继续？(y/n)${NC}"
            read -r answer
            if [ "$answer" != "y" ]; then
                echo -e "${RED}❌ 发布取消${NC}"
                exit 1
            fi
        fi
    fi
else
    echo -e "${YELLOW}⚠️  未找到版本测试脚本，跳过${NC}"
fi

echo ""

# ============================================================
# 步骤 3: 提交代码
# ============================================================
echo -e "${CYAN}═══════════════════════════════════════════════════════════${NC}"
echo -e "${CYAN}📦 步骤 3: 提交代码${NC}"
echo -e "${CYAN}═══════════════════════════════════════════════════════════${NC}"
echo ""

if [ "$AUTO_COMMIT" = true ] || [ -n "$(git status --porcelain)" ]; then
    echo -e "${BLUE}📝 提交更改...${NC}"
    git add .
    git commit -m "chore: release $VERSION

$MESSAGE

- Auto-generated by auto-release.sh"
    echo -e "${GREEN}✅ 代码已提交${NC}"
else
    echo -e "${GREEN}✅ 没有需要提交的更改${NC}"
fi

echo ""

# ============================================================
# 步骤 4: 创建标签
# ============================================================
echo -e "${CYAN}═══════════════════════════════════════════════════════════${NC}"
echo -e "${CYAN}📦 步骤 4: 创建 Git 标签${NC}"
echo -e "${CYAN}═══════════════════════════════════════════════════════════${NC}"
echo ""

echo -e "${BLUE}🏷️  创建标签 $VERSION...${NC}"

# 读取 RELEASE_TEST_REPORT.md 作为标签说明
TAG_MESSAGE="Release $VERSION: $MESSAGE"
if [ -f "RELEASE_TEST_REPORT.md" ]; then
    TAG_MESSAGE="$TAG_MESSAGE

## 发布验证报告

详见 RELEASE_TEST_REPORT.md"
fi

git tag -a "$VERSION" -m "$TAG_MESSAGE"
echo -e "${GREEN}✅ 标签已创建${NC}"

# 显示标签信息
echo ""
echo -e "${BLUE}📋 标签信息:${NC}"
git show "$VERSION" --stat | head -15
echo ""

# ============================================================
# 步骤 5: 推送到远程（最后一步）
# ============================================================
echo -e "${CYAN}═══════════════════════════════════════════════════════════${NC}"
echo -e "${CYAN}📦 步骤 5: 推送到远程仓库（最后一步）${NC}"
echo -e "${CYAN}═══════════════════════════════════════════════════════════${NC}"
echo ""

# 推送代码
echo -e "${BLUE}📤 推送代码...${NC}"
git push origin "$(git branch --show-current)"
echo -e "${GREEN}✅ 代码已推送${NC}"

# 推送标签
echo -e "${BLUE}📤 推送标签 $VERSION...${NC}"
git push origin "$VERSION"
echo -e "${GREEN}✅ 标签已推送${NC}"

echo ""
echo -e "${GREEN}✅ Git 推送完成！${NC}"
echo -e "${YELLOW}💡 线上 CI/CD 将自动触发构建和部署${NC}"

echo ""

# ============================================================
# 发布完成
# ============================================================
echo -e "${CYAN}╔═══════════════════════════════════════════════════════════╗${NC}"
echo -e "${CYAN}║                   🎉 发布完成！${NC}"
echo -e "${CYAN}╚═══════════════════════════════════════════════════════════╝${NC}"
echo ""

echo -e "${GREEN}✅ 发布成功！${NC}"
echo ""
echo -e "${BLUE}📋 发布信息:${NC}"
echo -e "  版本: ${GREEN}$VERSION${NC}"
echo -e "  说明: $MESSAGE"
echo -e "  分支: $(git branch --show-current)"
echo -e "  提交: $(git rev-parse --short HEAD)"
echo ""

echo -e "${BLUE}🔗 相关链接:${NC}"
REPO_URL=$(git remote get-url origin | sed 's/git@\(.*\):\(.*\)\.git/https:\/\/\1\/\2/')
echo -e "  GitHub: ${REPO_URL}/releases/tag/$VERSION"
echo ""

echo -e "${BLUE}📝 后续操作:${NC}"
echo "  1. 等待线上 CI/CD 自动构建和部署"
echo "  2. 编辑 GitHub Release 说明"
echo "     访问: ${REPO_URL}/releases/tag/$VERSION"
echo "  3. 通知团队"
echo ""

echo -e "${GREEN}🎊 发布完成！${NC}"
