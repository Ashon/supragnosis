#!/usr/bin/env bash
# supragnosis standalone 데몬 설치 (macOS/launchd).
# - 릴리스 바이너리를 안정 경로(~/.local/bin)로 복사(cargo clean 에도 안 깨지게)
# - LaunchAgent plist 설치 + 로드(로그인 시 자동 기동, 죽으면 재시작)
# - Claude Code 를 http 전송으로 재등록
# 실행: bash deploy/install.sh
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN_SRC="$REPO_ROOT/target/release/supragnosis"
BIN_DST="$HOME/.local/bin/supragnosis"
PLIST_SRC="$REPO_ROOT/deploy/launchd/com.ashon.supragnosis.plist"
PLIST_DST="$HOME/Library/LaunchAgents/com.ashon.supragnosis.plist"
MCP_URL="http://127.0.0.1:7373/mcp"

echo "[1/5] 릴리스 빌드"
( cd "$REPO_ROOT" && cargo build --release --bin supragnosis )

echo "[2/5] 기존 데몬 정지 (db lock/실행 파일 점유 해제)"
mkdir -p "$HOME/.local/bin" "$HOME/.supragnosis/db" "$HOME/.supragnosis/log"
# 교체 전에 먼저 멈춘다 - 실행 중 파일을 덮어쓰면 매핑이 깨진다. launchd 관리 프로세스는
# unload 로, 혹시 남은 것은 pkill 로(설치 경로 기준 - 데몬은 $BIN_DST 에서 돈다).
launchctl unload "$PLIST_DST" 2>/dev/null || true
pkill -f "$BIN_DST" 2>/dev/null || true

echo "[3/5] 바이너리 설치 + LaunchAgent 로드"
# 제자리 덮어쓰기(cp over)는 macOS 코드서명 캐시 불일치로 exec 시 SIGKILL('killed: 9')을
# 유발한다 - 새 inode 로 교체해 회피한다(rm 후 cp).
rm -f "$BIN_DST"
cp "$BIN_SRC" "$BIN_DST"
cp "$PLIST_SRC" "$PLIST_DST"
launchctl load "$PLIST_DST"
sleep 1

echo "[4/5] 헬스 체크 ($MCP_URL)"
# initialize 없이 GET 하면 405/이벤트지만, 포트가 열렸는지만 확인.
if curl -s -o /dev/null -m 3 "http://127.0.0.1:7373/mcp" ; then echo "  MCP 포트 응답 OK"; else echo "  (아직 준비 중일 수 있음 - 로그 확인)"; fi

echo "[5/5] Claude Code 를 http 전송으로 등록"
claude mcp remove supragnosis -s user 2>/dev/null || true
claude mcp add --transport http supragnosis "$MCP_URL" --scope user

echo ""
echo "완료. 뷰어: http://127.0.0.1:7374 | 로그: ~/.supragnosis/log/"
echo "중지:  launchctl unload $PLIST_DST"
echo "재시작: launchctl unload $PLIST_DST && launchctl load $PLIST_DST"
