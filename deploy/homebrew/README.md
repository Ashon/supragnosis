# Homebrew 배포 (formula + cask, DMG 없음)

이 디렉터리는 tap 리포로 복사해 쓰는 템플릿이다. 구성:

- `Formula/supragnosis-server.rb` - 서버/CLI (설치되는 바이너리 이름은 `supragnosis` 그대로,
  brew 토큰만 `-server`). 릴리스의 플랫폼별 tar.gz 를 그대로 설치하고,
  `brew services start supragnosis-server` 로 상시 데몬(launchd)을 등록한다. `serve --http` 는
  뷰어 소켓(`~/.supragnosis/viz.sock`)도 기본으로 연다.
- `Casks/supragnosis.rb` - 데스크탑 셸. 대표 토큰이라 `brew install supragnosis` 가 이 cask 로
  해석된다(동명 formula 없음). 릴리스의 서명/노터라이즈된 universal `.app.zip` 을 설치한다.
  cask 가 `supragnosis-server` formula 에 의존하므로 앱은 PATH 의 brew 데몬 바이너리를
  찾아 쓴다(sidecar 내장 없음). 앱이 트레이 상주형이라 cask 의 `uninstall quit:` 이
  업그레이드 시 구 인스턴스를 종료하고 다시 열어 준다.
- `update-tap.sh` - 릴리스 후 tap 의 version/sha256 을 릴리스 자산의 .sha256 사이드카에서
  받아 갱신한다.

## 최초 설정 (1회)

1. tap 리포 생성: GitHub 에 `Ashon/homebrew-tap` (public) 을 만들고 이 디렉터리의
   `Formula/`, `Casks/`, `update-tap.sh` 를 복사해 커밋한다.
2. 리포 시크릿 등록 (Settings > Secrets and variables > Actions) - release.yml 의 app 잡이
   서명/노터라이즈에 사용한다. 하나라도 없으면(정확히는 APPLE_SIGNING_IDENTITY 부재)
   서명 없이 빌드만 검증한다.
   - `APPLE_CERTIFICATE` - Developer ID Application 인증서 .p12 의 base64
     (`base64 -i cert.p12 | pbcopy`)
   - `APPLE_CERTIFICATE_PASSWORD` - .p12 암호
   - `APPLE_SIGNING_IDENTITY` - 예: `Developer ID Application: <Name> (<TEAMID>)`
   - `APPLE_ID` - Apple ID 이메일
   - `APPLE_PASSWORD` - app-specific password (appleid.apple.com 에서 발급)
   - `APPLE_TEAM_ID` - 팀 ID
3. tap 자동 갱신용 시크릿 등록: fine-grained PAT 를 만들어(Developer settings > Personal
   access tokens > Fine-grained, 대상 리포를 `Ashon/homebrew-tap` 하나로 제한, 권한은
   Contents: Read and write 만) `TAP_PUSH_TOKEN` 이름으로 리포 시크릿에 넣는다.
   없으면 release.yml 의 tap 잡이 건너뛰고 아래 수동 절차로 폴백한다.
4. 다음 `v*` 태그부터 릴리스에 `Supragnosis-v<ver>-macos-universal.app.zip` 이 첨부된다.

## 릴리스마다

자동이다: `v*` 태그를 push 하면 release.yml 의 tap 잡이 자산 첨부 완료 후 update-tap.sh 로
tap 의 version/sha256 을 갱신해 push 한다. 잡이 실패하거나 `TAP_PUSH_TOKEN` 이 없으면
수동으로 돌린다:

```sh
git clone git@github.com:Ashon/homebrew-tap && cd homebrew-tap
../supragnosis/deploy/homebrew/update-tap.sh v0.1.11 .
git commit -am "supragnosis v0.1.11" && git push
```

## 사용자 설치

```sh
brew tap ashon/tap
brew install supragnosis                # 데스크탑 앱 (macOS, server formula 포함)
brew install supragnosis-server         # 서버/CLI 만 (macOS / Linux)
brew services start supragnosis-server  # 상시 데몬 (MCP :7373 + viewer socket)
```

## 개발 버전 설치 (--HEAD)

formula 의 `head` 스펙이 main 브랜치를 소스 빌드한다 (rust 툴체인은 build dep 으로 자동,
기본 피처 = 키워드 검색으로 릴리스 바이너리와 동일). 뷰어 UI 는 서버 바이너리에 내장이므로
stable 데스크탑 앱 셸 + HEAD 서버 조합이 그대로 동작한다.

```sh
# stable 이 설치돼 있으면 formula 만 교체 (cask 의존성 경고는 --ignore-dependencies 로 통과)
brew services stop supragnosis-server
brew uninstall --ignore-dependencies supragnosis-server
brew install --HEAD supragnosis-server
brew services start supragnosis-server

brew upgrade --fetch-HEAD supragnosis-server   # 이후 main 이 갱신될 때마다
```

stable 로 복귀는 같은 절차에서 `brew install supragnosis-server` (--HEAD 없이).
버전 문자열이 `HEAD-<sha>` 로 찍히므로 어느 커밋을 쓰는 중인지 `brew info` 로 확인된다.
데이터 호환 주의: 개발 버전이 스키마/id 공식을 바꾼 경우 stable 복귀 전에 릴리스 노트의
migrate 안내를 확인한다 (`~/.supragnosis/db` 는 공유된다).

업그레이드는 `brew upgrade` 후 데몬 재시작까지 해야 완료된다 - brew upgrade 는 실행 중인
서비스를 재시작하지 않으므로(formula caveats 가 같은 안내를 출력), 재시작 없이는 구 데몬이
삭제된 keg 경로에서 계속 돈다:

```sh
brew upgrade
brew services restart supragnosis-server
```

구 토큰(formula `supragnosis`, cask `supragnosis-app`)으로 설치했다면 재설치한다.
서비스 중지가 uninstall 보다 먼저다 - brew uninstall 은 실행 중인 서비스/launchd plist 를
정리하지 않는다:

```sh
brew services stop supragnosis 2>/dev/null
brew uninstall --cask supragnosis-app 2>/dev/null; brew uninstall --formula supragnosis 2>/dev/null
brew install supragnosis
```

주의: `formula_renames.json` 으로 구 formula 토큰을 넘기지 않는다 - plain 토큰이 formula 이름으로
다시 해석되어 `brew install supragnosis` 가 cask 대신 formula 로 풀리는 것을 막기 위함이다.
