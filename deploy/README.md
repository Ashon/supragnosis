# supragnosis standalone 데몬 운용 (macOS)

chat 마다 stdio 로 스폰하는 대신, **상시 떠 있는 로컬 데몬 하나**가 db 를 잡고 MCP
streamable-http 를 노출한다. 에이전트(Claude Code 등)는 http 로 접속만 한다. 데몬이 db 의
유일한 보유자라 cozo 단일 프로세스 lock 문제도 사라진다.

- MCP: `http://127.0.0.1:7373/mcp` (loopback 전용, 무인증 = 로컬 신뢰 표면, 원칙 17)
- 뷰어: `http://127.0.0.1:7374`
- 모두 localhost. 비로컬 노출/인증/TLS 는 아직 미지원(후속).

## 빠른 설치 (권장)

```sh
bash deploy/install.sh
```

이 스크립트가: 릴리스 빌드 -> 바이너리를 `~/.local/bin/supragnosis` 로 복사 ->
LaunchAgent 설치+로드 -> Claude Code 를 http 전송으로 재등록 한다.

## 수동 설치

```sh
# 1) 빌드 + 바이너리를 안정 경로로(cargo clean 에도 안 깨지게)
cargo build --release --bin supragnosis
mkdir -p ~/.local/bin ~/.supragnosis/db ~/.supragnosis/log
cp target/release/supragnosis ~/.local/bin/supragnosis

# 2) 기존 stdio 서버가 db lock 을 쥐고 있으면 정리
pkill -f "target/release/supragnosis" || true

# 3) LaunchAgent 설치 + 로드(로그인 시 자동 기동 + 죽으면 재시작)
cp deploy/launchd/com.ashon.supragnosis.plist ~/Library/LaunchAgents/
launchctl load ~/Library/LaunchAgents/com.ashon.supragnosis.plist

# 4) Claude Code 를 http 전송으로 등록(더 이상 chat 마다 스폰 안 함)
claude mcp remove supragnosis -s user 2>/dev/null || true
claude mcp add supragnosis --transport http http://127.0.0.1:7373/mcp --scope user
```

이제 어느 chat/세션이든 이 데몬에 붙는다. 뷰어는 브라우저로 `http://127.0.0.1:7374`.

## 운용

```sh
# 상태/로그
launchctl list | grep supragnosis
tail -f ~/.supragnosis/log/supragnosis.err.log

# 중지 / 재시작
launchctl unload ~/Library/LaunchAgents/com.ashon.supragnosis.plist
launchctl load   ~/Library/LaunchAgents/com.ashon.supragnosis.plist

# 완전 제거
launchctl unload ~/Library/LaunchAgents/com.ashon.supragnosis.plist
rm ~/Library/LaunchAgents/com.ashon.supragnosis.plist
claude mcp remove supragnosis -s user
```

## 참고 / 주의

- 코드 갱신 후에는 `cargo build --release` + `cp target/release/supragnosis ~/.local/bin/` +
  데몬 재시작(위) 을 다시 하면 된다(`install.sh` 재실행이 가장 간단).
- plist 의 경로는 사용자 `ashon.lee` 기준 절대경로다. 다른 사용자는 plist 안의 `/Users/...`
  와 `Label`, `deploy/install.sh` 의 경로를 조정.
- 임베더는 `hashing`(다운로드 0, 결정적). 실제 의미 임베딩을 쓰려면 `--features fastembed`
  로 빌드하고 plist 의 `SUPRAGNOSIS_EMBED=fastembed` + **새 `SUPRAGNOSIS_DATA_DIR`** 로 바꾼다
  (기존 db 는 hashing-256 으로 색인돼 임베더 교체가 거부된다).
- 데몬은 하나만 떠야 한다(db + 포트 단일 점유). stdio 등록과 http 등록을 동시에 쓰지 말 것.
