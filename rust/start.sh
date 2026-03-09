#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────
#  Symphony 启动脚本
#  用法:
#    ./start.sh              — 编译并启动后端 + 前端 dev server
#    ./start.sh backend      — 仅启动后端
#    ./start.sh frontend     — 仅启动前端 dev server
#    ./start.sh build        — 编译前端并以生产模式启动后端（serve 静态文件）
#    ./start.sh check        — 仅检查编译，不运行
#
#  环境变量:
#    WORKFLOW_PATH  — WORKFLOW.md 路径 (默认: ./WORKFLOW.md)
#    PORT           — HTTP 服务端口 (默认: 不指定，使用 WORKFLOW.md 中 server.port)
#    RUST_LOG       — 日志级别 (默认: info)
# ─────────────────────────────────────────────────────────────
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FRONTEND_DIR="${SCRIPT_DIR}/frontend"

# ── 颜色 ──
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

info()  { echo -e "${BLUE}[INFO]${NC}  $*"; }
ok()    { echo -e "${GREEN}[OK]${NC}    $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC}  $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*"; }

# ── 前置检查 ──
check_prerequisites() {
    local missing=0

    if ! command -v cargo &>/dev/null; then
        error "未找到 cargo，请安装 Rust: https://rustup.rs"
        missing=1
    fi

    if ! command -v node &>/dev/null; then
        warn "未找到 node，前端功能将不可用"
    fi

    if ! command -v npm &>/dev/null; then
        warn "未找到 npm，前端功能将不可用"
    fi

    if [[ $missing -eq 1 ]]; then
        exit 1
    fi
}

# ── 编译后端 ──
build_backend() {
    info "编译 Rust 后端..."
    (cd "$SCRIPT_DIR" && cargo build --release)
    ok "后端编译完成"
}

# ── 安装前端依赖 ──
install_frontend_deps() {
    if [[ ! -d "${FRONTEND_DIR}/node_modules" ]]; then
        info "安装前端依赖..."
        (cd "$FRONTEND_DIR" && npm install)
        ok "前端依赖安装完成"
    fi
}

# ── 编译前端 ──
build_frontend() {
    install_frontend_deps
    info "编译前端..."
    (cd "$FRONTEND_DIR" && npm run build)
    ok "前端编译完成 → ${FRONTEND_DIR}/dist"
}

# ── 构建后端运行参数 ──
build_backend_args() {
    local args=()

    local wf="${WORKFLOW_PATH:-./WORKFLOW.md}"
    args+=("$wf")

    if [[ -n "${PORT:-}" ]]; then
        args+=("--port" "$PORT")
    fi

    echo "${args[@]}"
}

# ── 启动后端 ──
start_backend() {
    local args
    args=$(build_backend_args)

    export RUST_LOG="${RUST_LOG:-info}"

    info "启动 Symphony 后端..."
    info "  WORKFLOW: ${WORKFLOW_PATH:-./WORKFLOW.md}"
    if [[ -n "${PORT:-}" ]]; then
        info "  端口:     ${PORT}"
    fi
    info "  日志级别: ${RUST_LOG}"
    echo ""

    (cd "$SCRIPT_DIR" && cargo run --release -- $args)
}

# ── 启动前端 dev server ──
start_frontend_dev() {
    if ! command -v npm &>/dev/null; then
        error "未找到 npm，无法启动前端"
        exit 1
    fi

    install_frontend_deps
    info "启动前端 dev server..."
    (cd "$FRONTEND_DIR" && npm run dev)
}

# ── 同时启动后端和前端 ──
start_all() {
    info "═══════════════════════════════════════"
    info "  Symphony — 启动后端 + 前端"
    info "═══════════════════════════════════════"
    echo ""

    # 启动后端（后台）
    export RUST_LOG="${RUST_LOG:-info}"
    local backend_args
    backend_args=$(build_backend_args)

    info "启动后端 (后台运行)..."
    (cd "$SCRIPT_DIR" && cargo run --release -- $backend_args) &
    BACKEND_PID=$!
    ok "后端 PID: ${BACKEND_PID}"

    # 等一秒让后端先启动
    sleep 1

    # 启动前端 dev server（后台）
    if command -v npm &>/dev/null; then
        install_frontend_deps
        info "启动前端 dev server (后台运行)..."
        (cd "$FRONTEND_DIR" && npm run dev) &
        FRONTEND_PID=$!
        ok "前端 PID: ${FRONTEND_PID}"
    else
        warn "npm 不可用，跳过前端"
        FRONTEND_PID=""
    fi

    echo ""
    info "═══════════════════════════════════════"
    ok "所有服务已启动！"
    info "  后端 PID: ${BACKEND_PID}"
    if [[ -n "${FRONTEND_PID:-}" ]]; then
        info "  前端 PID: ${FRONTEND_PID}"
    fi
    info "  按 Ctrl+C 停止所有服务"
    info "═══════════════════════════════════════"

    # 捕获 Ctrl+C，优雅退出
    trap cleanup SIGINT SIGTERM

    # 等待任意子进程退出
    wait
}

# ── 清理函数 ──
cleanup() {
    echo ""
    info "正在停止所有服务..."

    if [[ -n "${BACKEND_PID:-}" ]] && kill -0 "$BACKEND_PID" 2>/dev/null; then
        kill "$BACKEND_PID" 2>/dev/null || true
        wait "$BACKEND_PID" 2>/dev/null || true
        ok "后端已停止"
    fi

    if [[ -n "${FRONTEND_PID:-}" ]] && kill -0 "$FRONTEND_PID" 2>/dev/null; then
        kill "$FRONTEND_PID" 2>/dev/null || true
        wait "$FRONTEND_PID" 2>/dev/null || true
        ok "前端已停止"
    fi

    ok "所有服务已停止"
    exit 0
}

# ── 主入口 ──
main() {
    check_prerequisites

    local cmd="${1:-all}"

    case "$cmd" in
        all)
            start_all
            ;;
        backend | back | b)
            build_backend
            start_backend
            ;;
        frontend | front | f)
            start_frontend_dev
            ;;
        build | prod)
            build_backend
            build_frontend
            info "生产构建完成。可以使用以下命令启动:"
            info "  cd ${SCRIPT_DIR} && RUST_LOG=info cargo run --release -- WORKFLOW.md"
            ;;
        check | c)
            info "检查 Rust 编译..."
            (cd "$SCRIPT_DIR" && cargo check)
            ok "编译检查通过"

            if command -v npm &>/dev/null && [[ -d "$FRONTEND_DIR" ]]; then
                info "检查前端 TypeScript..."
                install_frontend_deps
                (cd "$FRONTEND_DIR" && npx tsc --noEmit)
                ok "前端类型检查通过"
            fi
            ;;
        help | -h | --help)
            echo ""
            echo -e "${CYAN}Symphony 启动脚本${NC}"
            echo ""
            echo "用法: $0 [command]"
            echo ""
            echo "命令:"
            echo "  all       同时启动后端和前端 (默认)"
            echo "  backend   仅启动后端 (别名: back, b)"
            echo "  frontend  仅启动前端 dev server (别名: front, f)"
            echo "  build     编译后端和前端 (生产模式)"
            echo "  check     仅检查编译，不运行 (别名: c)"
            echo "  help      显示帮助信息"
            echo ""
            echo "环境变量:"
            echo "  WORKFLOW_PATH  WORKFLOW.md 路径 (默认: ./WORKFLOW.md)"
            echo "  PORT           HTTP 服务端口"
            echo "  RUST_LOG       日志级别 (默认: info)"
            echo ""
            echo "示例:"
            echo "  ./start.sh                           # 启动全部"
            echo "  ./start.sh backend                   # 仅后端"
            echo "  PORT=8080 ./start.sh backend         # 指定端口启动后端"
            echo "  RUST_LOG=debug ./start.sh            # debug 级别日志"
            echo "  WORKFLOW_PATH=~/my/WORKFLOW.md ./start.sh  # 自定义 workflow"
            echo ""
            ;;
        *)
            error "未知命令: $cmd"
            echo "使用 '$0 help' 查看帮助"
            exit 1
            ;;
    esac
}

main "$@"
