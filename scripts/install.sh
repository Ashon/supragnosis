#!/usr/bin/env sh
# supragnosis 설치 부트스트랩: 플랫폼을 감지해 최신 GitHub Release 바이너리를
# ~/.local/bin 에 설치한다.
#
#   curl -fsSL https://raw.githubusercontent.com/Ashon/supragnosis/main/scripts/install.sh | sh
#
# 환경변수:
#   SUPRAGNOSIS_VERSION  설치할 태그 (기본: latest)
#   BIN_DIR              설치 경로 (기본: ~/.local/bin)
#
# prebuilt 는 기본 빌드(키워드 + hashing 검색)다. 로컬 ONNX 의미 검색이 필요하면
# 소스에서 `cargo build --release --features fastembed` 로 빌드한다.
set -eu

REPO="Ashon/supragnosis"
BIN_DIR="${BIN_DIR:-$HOME/.local/bin}"
VERSION="${SUPRAGNOSIS_VERSION:-latest}"

usage() {
  cat <<'USAGE'
supragnosis 설치 스크립트

플랫폼(macOS arm64/x86_64, Linux x86_64)을 감지해 GitHub Release 바이너리를 설치한다
(sha256 체크섬 검증). 기본 설치 경로는 ~/.local/bin.

사용법:
  curl -fsSL https://raw.githubusercontent.com/Ashon/supragnosis/main/scripts/install.sh | sh
  curl -fsSL .../install.sh | sh -s -- [옵션]     # 파이프로 옵션 전달
  sh scripts/install.sh [옵션]                     # 로컬 파일로

옵션:
  -h, --help          이 도움말 출력
  -v, --version TAG   설치할 릴리스 태그 (기본: latest, 예: v0.1.0)
  -d, --dir DIR       설치 경로 (기본: ~/.local/bin)

환경변수(옵션이 우선):
  SUPRAGNOSIS_VERSION   설치할 태그
  BIN_DIR               설치 경로

예:
  sh scripts/install.sh --version v0.1.0
  BIN_DIR=/usr/local/bin sh scripts/install.sh

참고: prebuilt 는 키워드 + hashing 검색이다. 로컬 ONNX 의미 검색은 소스에서
  cargo build --release --features fastembed 로 빌드한다.
USAGE
}

# 옵션 파싱(환경변수 기본값을 덮어쓴다). 파이프 실행 시엔 `sh -s -- --help` 로 전달.
while [ $# -gt 0 ]; do
  case "$1" in
    -h|--help)     usage; exit 0 ;;
    -v|--version)  VERSION="${2:?--version 에 태그가 필요합니다}"; shift 2 ;;
    -d|--dir)      BIN_DIR="${2:?--dir 에 경로가 필요합니다}"; shift 2 ;;
    *) echo "알 수 없는 옵션: $1" >&2; echo >&2; usage >&2; exit 2 ;;
  esac
done

os="$(uname -s)"
arch="$(uname -m)"
case "${os}-${arch}" in
  Darwin-arm64)   target="aarch64-apple-darwin" ;;
  Darwin-x86_64)  target="x86_64-apple-darwin" ;;
  Linux-x86_64)   target="x86_64-unknown-linux-gnu" ;;
  *)
    echo "지원하지 않는 플랫폼: ${os}-${arch}" >&2
    echo "소스 빌드로 설치하세요: https://github.com/${REPO} (cargo build --release)" >&2
    exit 1
    ;;
esac

# 최신 릴리스 태그 조회(또는 지정 버전 사용).
if [ "${VERSION}" = "latest" ]; then
  VERSION="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep -m1 '"tag_name"' | cut -d'"' -f4)"
  [ -n "${VERSION}" ] || { echo "최신 릴리스를 찾지 못했습니다." >&2; exit 1; }
fi

name="supragnosis-${VERSION}-${target}"
url="https://github.com/${REPO}/releases/download/${VERSION}/${name}.tar.gz"

tmp="$(mktemp -d)"
trap 'rm -rf "${tmp}"' EXIT INT TERM

echo "다운로드: ${url}"
curl -fsSL "${url}" -o "${tmp}/pkg.tar.gz"
curl -fsSL "${url}.sha256" -o "${tmp}/pkg.sha256" 2>/dev/null || true

# 체크섬 검증(sha256 파일이 있으면).
if [ -s "${tmp}/pkg.sha256" ]; then
  want="$(cut -d' ' -f1 "${tmp}/pkg.sha256")"
  if command -v sha256sum >/dev/null 2>&1; then
    got="$(sha256sum "${tmp}/pkg.tar.gz" | cut -d' ' -f1)"
  else
    got="$(shasum -a 256 "${tmp}/pkg.tar.gz" | cut -d' ' -f1)"
  fi
  if [ "${want}" != "${got}" ]; then
    echo "체크섬 불일치 (기대 ${want}, 실제 ${got})." >&2
    exit 1
  fi
  echo "체크섬 OK"
fi

tar -C "${tmp}" -xzf "${tmp}/pkg.tar.gz"
mkdir -p "${BIN_DIR}"
install -m 0755 "${tmp}/${name}/supragnosis" "${BIN_DIR}/supragnosis"
echo "설치 완료: ${BIN_DIR}/supragnosis (${VERSION}, ${target})"

# PATH 안내.
case ":${PATH}:" in
  *":${BIN_DIR}:"*) : ;;
  *) echo "주의: ${BIN_DIR} 가 PATH 에 없습니다. 셸 설정(예: ~/.zshrc)에 추가하세요:"
     echo "  export PATH=\"${BIN_DIR}:\$PATH\"" ;;
esac

cat <<EOF

설치 완료. 온보딩:

  1) MCP 클라이언트(Claude Code 등)에 stdio 로 등록
       claude mcp add supragnosis -- "${BIN_DIR}/supragnosis"

  2) (선택) 상시 데몬 + 라이브 뷰어로 실행
       SUPRAGNOSIS_HTTP_ADDR=127.0.0.1:7373 SUPRAGNOSIS_VIZ_ADDR=127.0.0.1:7374 "${BIN_DIR}/supragnosis"
       # MCP: http://127.0.0.1:7373/mcp   뷰어: http://127.0.0.1:7374
       # 로그인 시 자동 기동(launchd)은 저장소 deploy/README.md 참고.

  - 검색: prebuilt 는 키워드/hashing. 의미 검색은 소스 --features fastembed 빌드.
  - 도움말: sh install.sh --help   |   문서/이슈: https://github.com/${REPO}
EOF
