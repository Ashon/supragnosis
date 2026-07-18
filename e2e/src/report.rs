//! 평가 산출물(마크다운 리포트, HTML 갤러리/뷰어, 데모 파일)의 출력 경로.
//! target/ 아래에 두어 저장소에는 실리지 않는다(저장소엔 완성물만).

use std::path::PathBuf;

/// target/eval-reports/ 를 만들어 돌려준다.
pub fn report_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/eval-reports");
    std::fs::create_dir_all(&dir).expect("리포트 디렉터리 생성");
    dir
}
