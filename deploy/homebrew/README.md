# Homebrew 배포 (formula + cask, DMG 없음)

이 디렉터리는 tap 리포로 복사해 쓰는 템플릿이다. 구성:

- `Formula/supragnosis.rb` - 데몬/CLI. 릴리스의 플랫폼별 tar.gz 를 그대로 설치하고,
  `brew services start supragnosis` 로 상시 데몬(launchd)을 등록한다. `serve --http` 는
  뷰어 소켓(`~/.supragnosis/viz.sock`)도 기본으로 연다.
- `Casks/supragnosis-app.rb` - 데스크탑 셸. 릴리스의 서명/노터라이즈된 universal
  `.app.zip` 을 설치한다. cask 가 formula 에 의존하므로 앱은 PATH 의 brew 데몬 바이너리를
  찾아 쓴다(sidecar 내장 없음). 업데이트는 `brew upgrade` 하나로 데몬+앱이 함께 올라간다.
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
3. 다음 `v*` 태그부터 릴리스에 `Supragnosis-v<ver>-macos-universal.app.zip` 이 첨부된다.

## 릴리스마다

```sh
git clone git@github.com:Ashon/homebrew-tap && cd homebrew-tap
../supragnosis/deploy/homebrew/update-tap.sh v0.1.10 .
git commit -am "supragnosis v0.1.10" && git push
```

## 사용자 설치

```sh
brew tap ashon/tap
brew install supragnosis            # 데몬/CLI
brew services start supragnosis     # 상시 데몬 (MCP :7373 + viewer socket)
brew install --cask supragnosis-app # 데스크탑 앱
```
