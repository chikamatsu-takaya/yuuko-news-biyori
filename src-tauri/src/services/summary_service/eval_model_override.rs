//! 評価ハーネス専用: 同梱と別のモデル（と llama-server）で評価を回すための差し替え（判断台帳 D168。テストビルド専用）。
//!
//! アプリ本体の同梱物・照合・起動の引数・プロンプトは変えない。差し替えるのは「起動する実行ファイルとモデルの
//! パス」と、候補ごとの調整（`ModelProfile`）だけで、どれも評価のテストからしか使われない。
//!
//! - 同梱物の照合（SHA-256）は同梱の Qwen3.5-2B と b11269 の部品に対して従来どおり行い、照合が通った後の
//!   起動だけを、環境変数で指したモデル・実行ファイルに向ける。候補のモデルの出所と SHA-256 は評価資料に記録する
//!   （`docs/00_project/ローカルLLMモデル比較の試作評価_2026-10.md`）。
//! - 起動の引数は同梱と同じ（`server_args`: CPU のみ・`-c 4096`・`-t`〔論理コア数の半分・上限 4〕・`-np 1`・
//!   `--cache-ram 0`・`--offline` など）。優先度も同じ「通常以下」。
//!
//! 環境変数（どれも省略可。`YUUKO_LLM_EVAL_MODEL` が無ければ同梱のまま動く）:
//! - `YUUKO_LLM_EVAL_MODEL`: 候補の GGUF のパス。
//! - `YUUKO_LLM_EVAL_SERVER_EXE`: 候補で使う llama-server.exe のパス（同梱の b11269 で動かないモデルだけ）。
//! - `YUUKO_LLM_EVAL_SERVER_LOG`: llama-server の標準エラーを書くファイル（読み込み失敗の原因調べ用）。
//! - `YUUKO_LLM_EVAL_PROFILE`: 候補ごとの調整の名前（`ModelProfile::from_name`）。省略時は調整なし。

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use crate::infra::local_llm_runtime::{
    no_window_command, server_args, LaunchSpec, ServerLauncher, ServerProcess,
};
use crate::services::local_llm_service::{LocalLlmConfig, LocalLlmService};

const MODEL_ENV: &str = "YUUKO_LLM_EVAL_MODEL";
const SERVER_EXE_ENV: &str = "YUUKO_LLM_EVAL_SERVER_EXE";
const SERVER_LOG_ENV: &str = "YUUKO_LLM_EVAL_SERVER_LOG";
const PROFILE_ENV: &str = "YUUKO_LLM_EVAL_PROFILE";

/// 評価で使う窓口。環境変数でモデルが指定されていれば、起動だけをそのモデルへ向ける。
pub(super) fn eval_local_llm(bundle_dir: PathBuf) -> LocalLlmService {
    let Some(model) = std::env::var_os(MODEL_ENV).map(PathBuf::from) else {
        return LocalLlmService::new(Some(bundle_dir));
    };
    assert!(model.is_file(), "{MODEL_ENV} must point to a GGUF file");
    let exe = std::env::var_os(SERVER_EXE_ENV).map(PathBuf::from);
    if let Some(exe) = &exe {
        assert!(exe.is_file(), "{SERVER_EXE_ENV} must point to llama-server");
    }
    let profile = ModelProfile::from_env();
    eprintln!(
        "eval model override: model={} exe={} profile={profile:?}",
        model.display(),
        exe.as_deref()
            .map_or_else(|| "bundled".to_string(), |exe| exe.display().to_string()),
    );
    let launcher = EvalLauncher {
        model,
        exe,
        log: std::env::var_os(SERVER_LOG_ENV).map(PathBuf::from),
        extra_args: profile.extra_server_args(),
    };
    LocalLlmService::with_parts(
        Some(bundle_dir),
        Arc::new(launcher),
        LocalLlmConfig::default(),
    )
}

/// Sarashina2.2 で使う chat テンプレート（GGUF に入っている版から道具〔tools〕の部分だけを除いたもの。
/// 利用者・system・assistant の発言の並べ方は元の版と1字も変わらない）。
const SARASHINA_TEMPLATE: &str =
    "src/services/summary_service/eval_data/sarashina2.2_chat_template_no_tools.jinja";

/// 候補ごとの調整。アプリの既定（プロンプト・温度・要求の形）は変えず、llama-server の起動の引数と、
/// 生出力の後始末（評価のテストの記録の前）だけをここに置く。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ModelProfile {
    /// 調整なし（同梱の Qwen3.5-2B と同じ扱い。Qwen3.5-4B・Gemma 4 E2B もこれで動いた）。
    Default,
    /// Sarashina2.2。GGUF の chat テンプレートが道具の呼び出しに対応した形のため、llama-server（b11269）が
    /// 出力を道具の形として読み、終わりの印 `</s>` を本文に残す（JSON スキーマで縛る要点の段では
    /// 「形が合わない」として 500 を返す）。道具の部分を除いたテンプレートを `--chat-template-file` で渡し、
    /// 念のため末尾に残った `</s>` も取り除く。
    Sarashina,
}

impl ModelProfile {
    pub(super) fn from_env() -> Self {
        Self::from_name(&std::env::var(PROFILE_ENV).unwrap_or_default())
    }

    fn from_name(name: &str) -> Self {
        match name {
            "" | "default" => Self::Default,
            "sarashina" => Self::Sarashina,
            other => panic!("unknown {PROFILE_ENV}: {other}"),
        }
    }

    fn extra_server_args(self) -> Vec<String> {
        match self {
            Self::Default => Vec::new(),
            Self::Sarashina => vec![
                "--chat-template-file".to_string(),
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join(SARASHINA_TEMPLATE)
                    .display()
                    .to_string(),
            ],
        }
    }

    /// 生出力の後始末（アプリの検証にかける前）。調整のない候補ではそのまま返す。
    pub(super) fn clean_raw(self, text: &str) -> String {
        match self {
            Self::Sarashina => {
                let mut text = text.trim_end();
                while let Some(rest) = text.strip_suffix("</s>") {
                    text = rest.trim_end();
                }
                text.to_string()
            }
            Self::Default => text.to_string(),
        }
    }
}

/// 同梱の起動（`ChildProcessLauncher`）と同じ引数・優先度で、モデルと実行ファイルだけを差し替えて起動する。
#[derive(Debug)]
struct EvalLauncher {
    model: PathBuf,
    exe: Option<PathBuf>,
    log: Option<PathBuf>,
    extra_args: Vec<String>,
}

impl ServerLauncher for EvalLauncher {
    fn launch(&self, spec: &LaunchSpec) -> std::io::Result<Box<dyn ServerProcess>> {
        let exe = self.exe.clone().unwrap_or_else(|| spec.bundle.exe.clone());
        let mut command = no_window_command(&exe);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(crate::infra::local_llm_runtime::server_creation_flags());
        }
        command
            .args(server_args(
                &self.model,
                spec.port.get(),
                &spec.api_key,
                spec.threads,
            ))
            .args(&self.extra_args)
            .stdin(Stdio::null())
            .stdout(Stdio::null());
        match &self.log {
            Some(path) => {
                let file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)?;
                command.stderr(file);
            }
            None => {
                command.stderr(Stdio::null());
            }
        }
        if let Some(parent) = exe.parent() {
            command.current_dir(parent);
        }
        Ok(Box::new(command.spawn()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_names_map_to_server_args() {
        assert!(ModelProfile::from_name("").extra_server_args().is_empty());
        assert!(ModelProfile::from_name("default")
            .extra_server_args()
            .is_empty());
        let args = ModelProfile::from_name("sarashina").extra_server_args();
        assert_eq!(args[0], "--chat-template-file");
        assert!(Path::new(&args[1]).is_file(), "{}", args[1]);
    }

    #[test]
    fn sarashina_cleanup_removes_only_the_trailing_marker() {
        let profile = ModelProfile::from_name("sarashina");
        assert_eq!(profile.clean_raw("要約です。</s>"), "要約です。");
        assert_eq!(
            profile.clean_raw(
                "要約です。 </s>
"
            ),
            "要約です。"
        );
        assert_eq!(profile.clean_raw("a</s>b"), "a</s>b");
        assert_eq!(
            ModelProfile::Default.clean_raw("要約です。</s>"),
            "要約です。</s>"
        );
    }
}
