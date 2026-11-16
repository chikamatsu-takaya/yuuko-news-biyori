//! 同梱ローカルLLMのモデル（GGUF）の取得の部品（設計案 §2.4・§2.5・§3.1・§3.2、
//! セキュリティ詳細設計書 §6.2・§6.3 の例外）。
//!
//! この層は「1 回の取得」だけを持つ。いつ始め・何回やり直し・どう見せるかは取得の制御
//! （`services::local_model_download_service`）が持つ。
//! - 取得先は `ModelSpec::source`（コードの表）から `url_guard` が作る URL だけ。任意の URL は受け取らない。
//! - 転送は自動で従わず、1 回ずつ `url_guard::model_redirect_url` で確かめる（最大 5 回）。
//! - 途中のファイルは `<file_name>.part`、記録は `<file_name>.part.json`。記録に転送先の URL は残さない。
//! - SHA-256 が合ったときだけ完成の名前へ変える。合わなければ 1 回だけ最初から取り直す。
//! - ログ・エラーに URL（転送先の署名を含む問い合わせ部分）・本文を出さない。エラーは固定の分類で返す。

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::net::ToSocketAddrs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::header::{CONTENT_RANGE, ETAG, IF_RANGE, LAST_MODIFIED, LOCATION, RANGE};
use reqwest::redirect::Policy;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

use super::atomic_file::{read_if_exists, write_json_atomic};
use super::local_llm_runtime::ModelSpec;
use super::url_guard::{
    check_model_download_addrs, model_download_url, model_redirect_url, ModelSource,
    MODEL_DOWNLOAD_MAX_REDIRECTS,
};

/// 接続（TLS を含む）の時間切れ（設計案 §2.4）。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// 何も届かない時間の上限（設計案 §2.4）。reqwest 0.12 の blocking の `timeout` は全体の時間ではなく、
/// 要求の送信と本文の読み取り 1 回ごとにかかる。そのため、これで「黙った相手」を切れる。
/// 止める合図は読み取り・ハッシュの読み直しの区切りごとに見る。合図が届かない区間は、読み取り 1 回の待ち
/// （最大この時間）・ヘッダーを待つ間（接続の 30 秒を含めて最大この時間）・OS の DNS の問い合わせの間（時間切れなし）。
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// 始める前に確かめる空きの余裕（設計案 §2.5）。
const FREE_SPACE_MARGIN: u64 = 1 << 30;
const READ_CHUNK: usize = 64 * 1024;
const USER_AGENT: &str = "yuuko-news-biyori/0.1";

/// 取得の失敗の分類（URL・パス・本文を含めない。画面の文言は React が決める）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadError {
    /// モデルの表に取得元が無い。
    NoSource,
    /// 取得先・転送先・DNS の答えの確認で止めた。
    Blocked,
    /// 通信の失敗・途中で切れた（`.part` は残すので続きから取れる）。
    Network,
    /// 想定外の HTTP 状態。
    HttpStatus(u16),
    /// 大きさが表と合わない・期待より大きい（`.part` は消す）。
    SizeMismatch,
    /// 取り直しても SHA-256 が合わない（`.part` は消す）。
    HashMismatch,
    /// 空きが足りない。`needed_more` はあと何バイト空ければよいか。
    InsufficientSpace { needed_more: u64 },
    /// 書き込み中に空きが無くなった（`.part` は残す）。
    DiskFull,
    /// そのほかのファイル操作の失敗。
    Io,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadOutcome {
    /// SHA-256 が合い、完成の名前になった。
    Completed,
    /// 止める合図で止まった（`.part` と記録は残す）。
    Stopped,
}

/// 取得の進み具合と、呼び出しをまたいで数える値。取得の制御が 1 つ持ち、呼び出しのたびに渡す。
#[derive(Default)]
pub struct DownloadProgress {
    /// `.part` に書いた大きさ（バイト）。
    pub received: AtomicU64,
    /// SHA-256 が合わなかった回数の通算。自動の取り直しは通算で 1 回だけ（呼び出しのたびに 1 回へ
    /// 戻ると、通信の失敗の後の自動のやり直しで、壊れたものを取り続けるため）。利用者の「やり直す」で 0 に戻す。
    pub hash_mismatches: AtomicU32,
    /// 書き進めたときに呼ぶ（取得の制御がイベントを間引いて送る）。
    pub on_progress: Option<Box<dyn Fn(u64) + Send + Sync>>,
}

impl DownloadProgress {
    fn report(&self, received: u64) {
        self.received.store(received, Ordering::Relaxed);
        if let Some(notify) = &self.on_progress {
            notify(received);
        }
    }
}

/// 1 回の要求で送る範囲（再開のとき）。
#[derive(Debug, Clone, Copy)]
pub(crate) struct RangeRequest<'a> {
    pub start: u64,
    pub if_range: Option<&'a str>,
}

/// 応答のうち、取得に要るものだけ。
pub(crate) struct HttpReply {
    pub status: u16,
    pub location: Option<String>,
    pub content_length: Option<u64>,
    pub content_range: Option<String>,
    /// `ETag`（強い比較のもの）か `Last-Modified`。`If-Range` に使う。
    pub validator: Option<String>,
    pub body: Box<dyn Read>,
}

/// 通信の差し替え口（テストで偽の応答を返すため）。本番は `ReqwestTransport` だけ。
pub(crate) trait ModelTransport {
    fn get(&self, url: &Url, range: Option<RangeRequest<'_>>) -> Result<HttpReply, DownloadError>;
}

/// `.part` の横の記録。転送先の URL・問い合わせ部分は持たない（期限切れになるうえ残したくないため）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PartRecord {
    file_name: String,
    size: u64,
    sha256: String,
    validator: Option<String>,
}

impl PartRecord {
    fn new(spec: &ModelSpec, validator: Option<String>) -> Self {
        Self {
            file_name: spec.file_name.to_string(),
            size: spec.size,
            sha256: spec.sha256.to_ascii_lowercase(),
            validator,
        }
    }

    fn is_for(&self, spec: &ModelSpec) -> bool {
        self.file_name == spec.file_name
            && self.size == spec.size
            && self.sha256.eq_ignore_ascii_case(spec.sha256)
    }
}

struct Paths {
    part: PathBuf,
    record: PathBuf,
    complete: PathBuf,
}

impl Paths {
    fn new(dir: &Path, spec: &ModelSpec) -> Self {
        Self {
            part: dir.join(format!("{}.part", spec.file_name)),
            record: dir.join(format!("{}.part.json", spec.file_name)),
            complete: dir.join(spec.file_name),
        }
    }

    /// 途中のファイルと記録を消す（無ければ何もしない）。
    fn discard(&self) -> Result<(), DownloadError> {
        for path in [&self.part, &self.record] {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(_) => return Err(DownloadError::Io),
            }
        }
        Ok(())
    }
}

enum Fetched {
    Stopped,
    /// `.part` が全体の大きさになった。受け取りながら計算した SHA-256 があれば持つ。
    Full(Option<[u8; 32]>),
}

/// モデルを `model_dir` に取得する（`model_dir` は取得の制御が置き場所の規則で決めたフォルダ）。
/// `stop` が立つと読み取りの区切りで止まる（一時停止・キャンセル・終了の区別は呼び出し側が持つ）。
pub fn download_model(
    spec: &ModelSpec,
    model_dir: &Path,
    stop: &AtomicBool,
    progress: &DownloadProgress,
) -> Result<DownloadOutcome, DownloadError> {
    let source = spec.source.ok_or(DownloadError::NoSource)?;
    download_with(
        spec,
        &source,
        model_dir,
        &ReqwestTransport,
        &available_space,
        stop,
        progress,
    )
}

fn download_with(
    spec: &ModelSpec,
    source: &ModelSource,
    model_dir: &Path,
    transport: &dyn ModelTransport,
    free_space: &dyn Fn(&Path) -> Option<u64>,
    stop: &AtomicBool,
    progress: &DownloadProgress,
) -> Result<DownloadOutcome, DownloadError> {
    fs::create_dir_all(model_dir).map_err(|_| DownloadError::Io)?;
    let paths = Paths::new(model_dir, spec);
    loop {
        let fetched = fetch_part(spec, source, &paths, transport, free_space, stop, progress)?;
        let digest = match fetched {
            Fetched::Stopped => return Ok(DownloadOutcome::Stopped),
            Fetched::Full(Some(digest)) => digest,
            Fetched::Full(None) => match hash_file(&paths.part, stop)? {
                Some(digest) => digest,
                None => return Ok(DownloadOutcome::Stopped),
            },
        };
        if hex(&digest).eq_ignore_ascii_case(spec.sha256) {
            fs::rename(&paths.part, &paths.complete).map_err(|_| DownloadError::Io)?;
            // 完成した後の記録は不要。消せなくても完成ファイルは正しいので失敗にしない。
            let _ = fs::remove_file(&paths.record);
            log::info!("local model download completed ({} bytes)", spec.size);
            return Ok(DownloadOutcome::Completed);
        }
        // 壊れたものを完成にしない。同じ取得元から取り続けないよう、自動の取り直しは 1 回だけ。
        paths.discard()?;
        progress.report(0);
        if progress.hash_mismatches.fetch_add(1, Ordering::Relaxed) >= 1 {
            log::warn!("local model download hash mismatch again; giving up");
            return Err(DownloadError::HashMismatch);
        }
        log::warn!("local model download hash mismatch; retrying once from the start");
    }
}

/// `.part` を全体の大きさまで進める（途中から・最初から）。
fn fetch_part(
    spec: &ModelSpec,
    source: &ModelSource,
    paths: &Paths,
    transport: &dyn ModelTransport,
    free_space: &dyn Fn(&Path) -> Option<u64>,
    stop: &AtomicBool,
    progress: &DownloadProgress,
) -> Result<Fetched, DownloadError> {
    let (mut have, mut validator) = prepare_part(spec, paths)?;
    progress.report(have);
    // 全部そろっているのに Range を送ると 416 になり、丸ごと取り直しになるため通信しない。
    if have == spec.size {
        return Ok(Fetched::Full(None));
    }
    check_free_space(free_space, paths, spec.size - have)?;

    let mut restarted = false;
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(Fetched::Stopped);
        }
        let range = (have > 0).then_some(RangeRequest {
            start: have,
            if_range: validator.as_deref(),
        });
        let reply = request_following_redirects(transport, source, range)?;
        let restart = match reply.status {
            206 if have > 0 => {
                let range_ok =
                    parse_content_range(reply.content_range.as_deref()) == Some((have, spec.size));
                // 記録した ETag と違う → 中身が変わった。続きをつなげない。
                let same_content = validator.is_none() || reply.validator == validator;
                if range_ok && same_content {
                    // `Content-Length` が無い 206（chunked など）は、`Content-Range` の始まりと全体の大きさが
                    // 合っていれば受け入れる。長さは受け取りの上限（全体を 1 バイトでも超えたら止めて消す）で守れ、
                    // 短ければ途中で切れた扱い（`.part` を残して次は続きから）になるため、取り終えた分を捨てて
                    // 最初から取り直すより損が少ない。宣言があって合わないときは従来どおり止めて消す。
                    if reply.content_length.is_some() {
                        expect_length(&reply, spec.size - have, paths)?;
                    }
                    false
                } else {
                    true
                }
            }
            // 空きは確かめ直さない: 始める前の確認は「空き ≥ 残り（全体 − `.part`）＋余裕」で、200 では
            // `.part` を消してから全体を書くため、要るのは「空き＋`.part` ≥ 全体＋余裕」となり同じ条件になる
            // （設計案 §7.1）。その後にほかの所が空きを使ったときは、書き込みの途中の `DiskFull` で止まる。
            200 => {
                expect_length(&reply, spec.size, paths)?;
                have = 0;
                false
            }
            416 => true,
            status => return Err(DownloadError::HttpStatus(status)),
        };
        if restart {
            paths.discard()?;
            if restarted {
                return Err(DownloadError::SizeMismatch);
            }
            log::info!("local model download cannot resume; starting over");
            restarted = true;
            have = 0;
            validator = None;
            continue;
        }
        validator = reply.validator.clone();
        if have == 0 {
            // 最初から書くときは、新しい記録より先に古い .part を消す
            // （古い中身の .part と新しい ETag の記録が組になって残る隙間を作らない）。
            paths.discard()?;
        }
        write_json_atomic(
            &paths.record,
            &PartRecord::new(spec, validator.clone()),
            "model download record",
        )
        .map_err(|_| DownloadError::Io)?;
        return stream_body(reply.body, paths, have, spec.size, stop, progress);
    }
}

/// 記録と `.part` を見て、続きから取れる大きさと `If-Range` の値を返す。
/// 記録が無い・壊れている・別のモデルのもの・`.part` が大きすぎる → 消して最初から。
fn prepare_part(spec: &ModelSpec, paths: &Paths) -> Result<(u64, Option<String>), DownloadError> {
    let part_len = fs::metadata(&paths.part).ok().map(|meta| meta.len());
    let record = read_if_exists(&paths.record)
        .ok()
        .flatten()
        .and_then(|bytes| serde_json::from_slice::<PartRecord>(&bytes).ok());
    match (part_len, record) {
        (Some(len), Some(record)) if record.is_for(spec) && len <= spec.size => {
            Ok((len, record.validator))
        }
        _ => {
            paths.discard()?;
            Ok((0, None))
        }
    }
}

fn check_free_space(
    free_space: &dyn Fn(&Path) -> Option<u64>,
    paths: &Paths,
    remaining: u64,
) -> Result<(), DownloadError> {
    let dir = paths.part.parent().unwrap_or(Path::new("."));
    let required = remaining.saturating_add(FREE_SPACE_MARGIN);
    match free_space(dir) {
        Some(free) if free < required => Err(DownloadError::InsufficientSpace {
            needed_more: required - free,
        }),
        Some(_) => Ok(()),
        // 空きが分からないときは止めない（書き込み中に満杯になれば DiskFull で止まり、.part は残る）。
        None => {
            log::warn!("could not read free disk space before model download");
            Ok(())
        }
    }
}

/// 宣言の大きさが期待と合うか。合わなければ始めずに `.part` を消す（設計案 §2.4）。
fn expect_length(reply: &HttpReply, expected: u64, paths: &Paths) -> Result<(), DownloadError> {
    if reply.content_length == Some(expected) {
        return Ok(());
    }
    paths.discard()?;
    Err(DownloadError::SizeMismatch)
}

/// 固定の取得元から始め、転送を 1 回ずつ確かめて進む。
fn request_following_redirects(
    transport: &dyn ModelTransport,
    source: &ModelSource,
    range: Option<RangeRequest<'_>>,
) -> Result<HttpReply, DownloadError> {
    let mut url = model_download_url(source).map_err(|_| DownloadError::Blocked)?;
    for hop in 0..=MODEL_DOWNLOAD_MAX_REDIRECTS {
        let reply = transport.get(&url, range)?;
        if !matches!(reply.status, 301 | 302 | 303 | 307 | 308) {
            return Ok(reply);
        }
        if hop == MODEL_DOWNLOAD_MAX_REDIRECTS {
            break;
        }
        let location = reply.location.as_deref().ok_or(DownloadError::Blocked)?;
        url = model_redirect_url(&url, location, source).map_err(|_| {
            // 許すホストを見直せるよう、弾いたホスト名だけを残す（経路・問い合わせ・署名は出さない）。
            log::warn!(
                "local model download redirect rejected (host: {})",
                rejected_redirect_host(&url, location)
            );
            DownloadError::Blocked
        })?;
    }
    log::warn!("local model download exceeded the redirect limit");
    Err(DownloadError::Blocked)
}

/// 弾いた転送先のログ用のホスト名。`Location` は外部由来なので、ホスト名だけを取り出し、
/// ホスト名に使う文字（英数字・`.`・`-`・`:`・`[`・`]`）以外は落として長さも抑える。読めなければ固定の印。
fn rejected_redirect_host(current: &Url, location: &str) -> String {
    let host: String = current
        .join(location.trim())
        .ok()
        .and_then(|next| next.host_str().map(str::to_string))
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'))
        .take(253)
        .collect();
    if host.is_empty() {
        "(none)".to_string()
    } else {
        host
    }
}

/// 本文を `.part` へ書き足しながら SHA-256 を計算する。再開のときは既存の `.part` を読み直して
/// ハッシュの状態を作る（途中の状態は保存しない。設計案 §2.4）。
fn stream_body(
    mut body: Box<dyn Read>,
    paths: &Paths,
    have: u64,
    size: u64,
    stop: &AtomicBool,
    progress: &DownloadProgress,
) -> Result<Fetched, DownloadError> {
    let mut hasher = Sha256::new();
    let mut file = if have == 0 {
        File::create(&paths.part).map_err(|_| DownloadError::Io)?
    } else {
        if !hash_into(&paths.part, have, &mut hasher, stop)? {
            return Ok(Fetched::Stopped);
        }
        OpenOptions::new()
            .append(true)
            .open(&paths.part)
            .map_err(|_| DownloadError::Io)?
    };
    let mut total = have;
    let mut buf = vec![0u8; READ_CHUNK];
    loop {
        if stop.load(Ordering::Relaxed) {
            file.flush().map_err(write_error)?;
            return Ok(Fetched::Stopped);
        }
        let read = match body.read(&mut buf) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(DownloadError::Network),
        };
        // 期待より 1 バイトでも多ければ、その場で止めて消す（大きすぎる応答でディスクを埋めない）。
        if total + read as u64 > size {
            drop(file);
            paths.discard()?;
            return Err(DownloadError::SizeMismatch);
        }
        file.write_all(&buf[..read]).map_err(write_error)?;
        hasher.update(&buf[..read]);
        total += read as u64;
        progress.report(total);
    }
    file.sync_all().map_err(write_error)?;
    if total < size {
        // 途中で切れた。.part と記録は残し、次は続きから取る。
        return Err(DownloadError::Network);
    }
    Ok(Fetched::Full(Some(hasher.finalize().into())))
}

fn write_error(error: io::Error) -> DownloadError {
    if error.kind() == io::ErrorKind::StorageFull {
        DownloadError::DiskFull
    } else {
        DownloadError::Io
    }
}

/// ファイルの先頭 `len` バイトをハッシュへ足す。3GB の読み直しは数秒かかりうるため、
/// 1MB ごとに止める合図を見る。止まったら false（.part と記録は残す）。
fn hash_into(
    path: &Path,
    len: u64,
    hasher: &mut Sha256,
    stop: &AtomicBool,
) -> Result<bool, DownloadError> {
    let file = File::open(path).map_err(|_| DownloadError::Io)?;
    let mut limited = file.take(len);
    let mut buf = vec![0u8; 1 << 20];
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(false);
        }
        match limited.read(&mut buf) {
            Ok(0) => return Ok(true),
            Ok(read) => hasher.update(&buf[..read]),
            Err(_) => return Err(DownloadError::Io),
        }
    }
}

/// ファイル全体の SHA-256。止まったら None。
fn hash_file(path: &Path, stop: &AtomicBool) -> Result<Option<[u8; 32]>, DownloadError> {
    let mut hasher = Sha256::new();
    if !hash_into(path, u64::MAX, &mut hasher, stop)? {
        return Ok(None);
    }
    Ok(Some(hasher.finalize().into()))
}

fn hex(digest: &[u8; 32]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `bytes <start>-<end>/<total>` から (start, total) を読む。
fn parse_content_range(value: Option<&str>) -> Option<(u64, u64)> {
    let rest = value?.trim().strip_prefix("bytes ")?;
    let (range, total) = rest.split_once('/')?;
    let (start, _end) = range.split_once('-')?;
    Some((start.trim().parse().ok()?, total.trim().parse().ok()?))
}

/// 本番の通信。要求ごとに DNS の答えを確かめ、その IP へ `resolve_to_addrs` で固定する
/// （確かめた後に答えを変えて内側へつながせる攻撃への対策。RSS・記事取得と同じ）。
struct ReqwestTransport;

/// 取得用のクライアントの設定。`timeout` は読み取りごとの時間切れなので（`IDLE_TIMEOUT`）、
/// 3GB の取得でも、データが届き続けている限り切れない。
fn client_builder() -> reqwest::blocking::ClientBuilder {
    Client::builder()
        .redirect(Policy::none())
        // 環境変数のプロキシを使わない（設計案 §8 の 10。藤井 2026-11-16）。プロキシを通すと、確かめた IP への
        // 固定（`resolve_to_addrs`）と拒否帯の確認が効かなくなるため。前例は local_llm_runtime の 127.0.0.1 への接続。
        .no_proxy()
        .timeout(IDLE_TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        .referer(false)
        .user_agent(USER_AGENT)
}

impl ModelTransport for ReqwestTransport {
    fn get(&self, url: &Url, range: Option<RangeRequest<'_>>) -> Result<HttpReply, DownloadError> {
        let host = url.host_str().ok_or(DownloadError::Blocked)?;
        let resolved = (host, 443).to_socket_addrs().map_err(|_| {
            log::warn!("local model download DNS lookup failed");
            DownloadError::Network
        })?;
        let addrs = check_model_download_addrs(resolved).map_err(|_| {
            log::warn!("local model download host resolved to a disallowed IP");
            DownloadError::Blocked
        })?;
        let client = client_builder()
            .resolve_to_addrs(host, &addrs)
            .build()
            .map_err(|_| DownloadError::Network)?;
        let mut request = client.get(url.clone());
        if let Some(range) = range {
            request = request.header(RANGE, format!("bytes={}-", range.start));
            if let Some(validator) = range.if_range {
                request = request.header(IF_RANGE, validator);
            }
        }
        let response = request.send().map_err(|error| {
            // reqwest のエラー文は URL を含むため、種類だけを残す。
            log::warn!(
                "local model download request failed (timeout: {}, connect: {})",
                error.is_timeout(),
                error.is_connect()
            );
            DownloadError::Network
        })?;
        let header = |name| {
            response
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string)
        };
        // 弱い ETag（W/）は If-Range に使えないため、そのときは Last-Modified を使う。
        let validator = header(ETAG)
            .filter(|etag| !etag.starts_with("W/"))
            .or_else(|| header(LAST_MODIFIED));
        Ok(HttpReply {
            status: response.status().as_u16(),
            location: header(LOCATION),
            content_length: response.content_length(),
            content_range: header(CONTENT_RANGE),
            validator,
            body: Box::new(response),
        })
    }
}

/// 置き場所のドライブの、この利用者が使える空き（バイト）。分からなければ None。
#[cfg(windows)]
fn available_space(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let wide: Vec<u16> = dir
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut free: u64 = 0;
    // SAFETY: `wide` は NUL 終端の UTF-16。受け取り先は有効な u64 で、使わない 2 つは null を渡せる。
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(free)
}

#[cfg(not(windows))]
fn available_space(_dir: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::sync::atomic::AtomicU64;
    use std::sync::Arc;

    const SOURCE: ModelSource = ModelSource {
        host: "models.example.test",
        path: "/repo/resolve/0123abcd/model.gguf",
        redirect_hosts: &["cdn.example.test"],
    };
    const BODY_LEN: usize = 200_000;
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn body() -> Vec<u8> {
        (0..BODY_LEN).map(|i| (i * 31 % 251) as u8).collect()
    }

    fn spec_for(content: &[u8]) -> ModelSpec {
        let digest: [u8; 32] = Sha256::digest(content).into();
        ModelSpec {
            file_name: "model.gguf",
            size: content.len() as u64,
            sha256: Box::leak(hex(&digest).into_boxed_str()),
            source: Some(SOURCE),
        }
    }

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "yuuko_model_download_{}_{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    struct Reply {
        status: u16,
        location: Option<&'static str>,
        length: Option<u64>,
        range: Option<String>,
        etag: Option<&'static str>,
        body: Vec<u8>,
    }

    fn ok(body: &[u8]) -> Reply {
        Reply {
            status: 200,
            location: None,
            length: Some(body.len() as u64),
            range: None,
            etag: Some("\"v1\""),
            body: body.to_vec(),
        }
    }

    fn partial(full: &[u8], start: usize, etag: &'static str) -> Reply {
        Reply {
            status: 206,
            location: None,
            length: Some((full.len() - start) as u64),
            range: Some(format!("bytes {start}-{}/{}", full.len() - 1, full.len())),
            etag: Some(etag),
            body: full[start..].to_vec(),
        }
    }

    fn status(status: u16, location: Option<&'static str>) -> Reply {
        Reply {
            status,
            location,
            length: Some(0),
            range: None,
            etag: None,
            body: Vec::new(),
        }
    }

    /// 受けた要求（URL・Range の始まり・If-Range）。
    type Requested = (String, Option<u64>, Option<String>);

    /// 偽の通信。要求（URL・Range の始まり・If-Range）を覚え、並べた応答を順に返す。
    #[derive(Default)]
    struct FakeTransport {
        replies: RefCell<VecDeque<Reply>>,
        requests: RefCell<Vec<Requested>>,
        /// 本文を読んだら立てる合図（読み取り中の停止の確認用）。
        stop_on_read: Option<Arc<AtomicBool>>,
    }

    impl FakeTransport {
        fn new(replies: Vec<Reply>) -> Self {
            Self {
                replies: RefCell::new(replies.into()),
                ..Self::default()
            }
        }
    }

    struct StopAfterRead(io::Cursor<Vec<u8>>, Arc<AtomicBool>);

    impl Read for StopAfterRead {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let limit = buf.len().min(1000);
            let read = self.0.read(&mut buf[..limit])?;
            self.1.store(true, Ordering::Relaxed);
            Ok(read)
        }
    }

    impl ModelTransport for FakeTransport {
        fn get(
            &self,
            url: &Url,
            range: Option<RangeRequest<'_>>,
        ) -> Result<HttpReply, DownloadError> {
            self.requests.borrow_mut().push((
                url.to_string(),
                range.map(|r| r.start),
                range.and_then(|r| r.if_range.map(str::to_string)),
            ));
            let reply = self
                .replies
                .borrow_mut()
                .pop_front()
                .expect("unexpected request");
            let cursor = io::Cursor::new(reply.body);
            let body: Box<dyn Read> = match &self.stop_on_read {
                Some(stop) => Box::new(StopAfterRead(cursor, stop.clone())),
                None => Box::new(cursor),
            };
            Ok(HttpReply {
                status: reply.status,
                location: reply.location.map(str::to_string),
                content_length: reply.length,
                content_range: reply.range,
                validator: reply.etag.map(str::to_string),
                body,
            })
        }
    }

    fn plenty(_: &Path) -> Option<u64> {
        Some(u64::MAX)
    }

    fn run(
        spec: &ModelSpec,
        dir: &Path,
        transport: &FakeTransport,
    ) -> Result<DownloadOutcome, DownloadError> {
        let stop = AtomicBool::new(false);
        download_with(
            spec,
            &SOURCE,
            dir,
            transport,
            &plenty,
            &stop,
            &DownloadProgress::default(),
        )
    }

    /// 途中までの `.part` と記録を置く。
    fn seed_part(dir: &Path, spec: &ModelSpec, content: &[u8], etag: Option<&str>) {
        let paths = Paths::new(dir, spec);
        fs::create_dir_all(dir).unwrap();
        fs::write(&paths.part, content).unwrap();
        let record = PartRecord::new(spec, etag.map(str::to_string));
        write_json_atomic(&paths.record, &record, "test").unwrap();
    }

    fn assert_completed(dir: &Path, spec: &ModelSpec, full: &[u8]) {
        let paths = Paths::new(dir, spec);
        assert_eq!(fs::read(&paths.complete).unwrap(), full);
        assert!(!paths.part.exists() && !paths.record.exists());
    }

    fn assert_nothing_left(dir: &Path, spec: &ModelSpec) {
        let paths = Paths::new(dir, spec);
        assert!(!paths.part.exists() && !paths.record.exists() && !paths.complete.exists());
    }

    #[test]
    fn downloads_from_the_start_with_the_fixed_url() {
        let (full, dir) = (body(), temp_dir());
        let spec = spec_for(&full);
        let transport = FakeTransport::new(vec![ok(&full)]);
        assert_eq!(run(&spec, &dir, &transport), Ok(DownloadOutcome::Completed));
        assert_completed(&dir, &spec, &full);
        let requests = transport.requests.borrow();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].0,
            "https://models.example.test/repo/resolve/0123abcd/model.gguf"
        );
        assert_eq!(requests[0].1, None);
    }

    #[test]
    fn resumes_with_range_and_if_range_on_206() {
        let (full, dir) = (body(), temp_dir());
        let spec = spec_for(&full);
        seed_part(&dir, &spec, &full[..70_000], Some("\"v1\""));
        let transport = FakeTransport::new(vec![partial(&full, 70_000, "\"v1\"")]);
        assert_eq!(run(&spec, &dir, &transport), Ok(DownloadOutcome::Completed));
        assert_completed(&dir, &spec, &full);
        let requests = transport.requests.borrow();
        assert_eq!(requests[0].1, Some(70_000));
        assert_eq!(requests[0].2.as_deref(), Some("\"v1\""));
    }

    #[test]
    fn starts_over_when_resume_is_answered_with_200() {
        let (full, dir) = (body(), temp_dir());
        let spec = spec_for(&full);
        seed_part(&dir, &spec, &[9u8; 50_000], Some("\"old\""));
        let transport = FakeTransport::new(vec![ok(&full)]);
        assert_eq!(run(&spec, &dir, &transport), Ok(DownloadOutcome::Completed));
        assert_completed(&dir, &spec, &full);
    }

    #[test]
    fn starts_over_on_416_wrong_content_range_or_changed_etag() {
        let full = body();
        let spec = spec_for(&full);
        let mut wrong_start = partial(&full, 70_000, "\"v1\"");
        wrong_start.range = Some(format!("bytes 60000-{}/{}", BODY_LEN - 1, BODY_LEN));
        for first in [
            status(416, None),
            wrong_start,
            partial(&full, 70_000, "\"v2\""),
        ] {
            let dir = temp_dir();
            seed_part(&dir, &spec, &full[..70_000], Some("\"v1\""));
            let transport = FakeTransport::new(vec![first, ok(&full)]);
            assert_eq!(run(&spec, &dir, &transport), Ok(DownloadOutcome::Completed));
            assert_completed(&dir, &spec, &full);
            // 2 回目は Range なし（最初から）。
            assert_eq!(transport.requests.borrow()[1].1, None);
        }
    }

    #[test]
    fn full_part_goes_to_verification_without_network() {
        let (full, dir) = (body(), temp_dir());
        let spec = spec_for(&full);
        seed_part(&dir, &spec, &full, Some("\"v1\""));
        let transport = FakeTransport::new(Vec::new());
        assert_eq!(run(&spec, &dir, &transport), Ok(DownloadOutcome::Completed));
        assert_completed(&dir, &spec, &full);
        assert!(transport.requests.borrow().is_empty());
    }

    #[test]
    fn stops_while_rehashing_the_part_and_keeps_it() {
        let (full, dir) = (body(), temp_dir());
        let spec = spec_for(&full);
        seed_part(&dir, &spec, &full, Some("\"v1\""));
        let transport = FakeTransport::new(Vec::new());
        let stop = AtomicBool::new(true);
        let outcome = download_with(
            &spec,
            &SOURCE,
            &dir,
            &transport,
            &plenty,
            &stop,
            &DownloadProgress::default(),
        );
        assert_eq!(outcome, Ok(DownloadOutcome::Stopped));
        let paths = Paths::new(&dir, &spec);
        assert_eq!(fs::read(&paths.part).unwrap(), full);
        assert!(paths.record.exists() && !paths.complete.exists());
        assert!(transport.requests.borrow().is_empty());

        // 再開の読み直し（続きを取る前）でも止まる。
        let mut hasher = Sha256::new();
        assert_eq!(hash_into(&paths.part, 10, &mut hasher, &stop), Ok(false));
    }

    #[test]
    fn part_with_a_record_for_another_model_is_discarded() {
        let (full, dir) = (body(), temp_dir());
        let spec = spec_for(&full);
        let other = ModelSpec {
            sha256: "00",
            ..spec
        };
        seed_part(&dir, &other, &full[..10], None);
        let transport = FakeTransport::new(vec![ok(&full)]);
        assert_eq!(run(&spec, &dir, &transport), Ok(DownloadOutcome::Completed));
        assert_eq!(transport.requests.borrow()[0].1, None);
    }

    #[test]
    fn rejects_wrong_declared_length_and_oversized_body() {
        let full = body();
        let spec = spec_for(&full);
        let mut short_200 = ok(&full);
        short_200.length = Some(BODY_LEN as u64 - 1);
        let mut wrong_206 = partial(&full, 70_000, "\"v1\"");
        wrong_206.length = Some(BODY_LEN as u64);
        let mut oversized = ok(&full);
        oversized.body.push(0);

        for (reply, seeded) in [(short_200, false), (wrong_206, true), (oversized, false)] {
            let dir = temp_dir();
            if seeded {
                seed_part(&dir, &spec, &full[..70_000], Some("\"v1\""));
            }
            let transport = FakeTransport::new(vec![reply]);
            assert_eq!(
                run(&spec, &dir, &transport),
                Err(DownloadError::SizeMismatch)
            );
            assert_nothing_left(&dir, &spec);
        }
    }

    /// `Content-Length` の無い 206 は、`Content-Range` が合えば続きを書く（取り終えた分を捨てない）。
    /// 大きすぎる本文は上限で止めて消し、短い本文は途中で切れた扱いで `.part` を残す。
    #[test]
    fn resumes_on_206_without_content_length_within_the_size_limit() {
        let full = body();
        let spec = spec_for(&full);
        let without_length = |extra: &[u8], cut: usize| {
            let mut reply = partial(&full, 70_000, "\"v1\"");
            reply.length = None;
            reply.body.truncate(reply.body.len() - cut);
            reply.body.extend_from_slice(extra);
            reply
        };

        let dir = temp_dir();
        seed_part(&dir, &spec, &full[..70_000], Some("\"v1\""));
        let transport = FakeTransport::new(vec![without_length(&[], 0)]);
        assert_eq!(run(&spec, &dir, &transport), Ok(DownloadOutcome::Completed));
        assert_completed(&dir, &spec, &full);
        assert_eq!(transport.requests.borrow().len(), 1);

        let dir = temp_dir();
        seed_part(&dir, &spec, &full[..70_000], Some("\"v1\""));
        let transport = FakeTransport::new(vec![without_length(&[0], 0)]);
        assert_eq!(
            run(&spec, &dir, &transport),
            Err(DownloadError::SizeMismatch)
        );
        assert_nothing_left(&dir, &spec);

        let dir = temp_dir();
        seed_part(&dir, &spec, &full[..70_000], Some("\"v1\""));
        let transport = FakeTransport::new(vec![without_length(&[], 30_000)]);
        assert_eq!(run(&spec, &dir, &transport), Err(DownloadError::Network));
        let paths = Paths::new(&dir, &spec);
        assert_eq!(fs::read(&paths.part).unwrap(), &full[..BODY_LEN - 30_000]);
        assert!(paths.record.exists() && !paths.complete.exists());
    }

    #[test]
    fn hash_mismatch_retries_once_then_fails_without_complete_file() {
        let full = body();
        let spec = spec_for(&full);
        let mut broken = full.clone();
        broken[0] ^= 1;

        let dir = temp_dir();
        let transport = FakeTransport::new(vec![ok(&broken), ok(&full)]);
        assert_eq!(run(&spec, &dir, &transport), Ok(DownloadOutcome::Completed));
        assert_completed(&dir, &spec, &full);

        let dir = temp_dir();
        let transport = FakeTransport::new(vec![ok(&broken), ok(&broken)]);
        assert_eq!(
            run(&spec, &dir, &transport),
            Err(DownloadError::HashMismatch)
        );
        assert_nothing_left(&dir, &spec);
        assert_eq!(transport.requests.borrow().len(), 2);

        // 前の呼び出しで 1 回合わなかった（通算 1）なら、次の呼び出しでは取り直さずに止める。
        let dir = temp_dir();
        let transport = FakeTransport::new(vec![ok(&broken)]);
        let progress = DownloadProgress::default();
        progress.hash_mismatches.store(1, Ordering::Relaxed);
        let stop = AtomicBool::new(false);
        let outcome = download_with(&spec, &SOURCE, &dir, &transport, &plenty, &stop, &progress);
        assert_eq!(outcome, Err(DownloadError::HashMismatch));
        assert_eq!(progress.hash_mismatches.load(Ordering::Relaxed), 2);
        assert_eq!(transport.requests.borrow().len(), 1);
    }

    #[test]
    fn follows_five_checked_redirects_and_keeps_signed_urls_out_of_the_record() {
        let (full, dir) = (body(), temp_dir());
        let spec = spec_for(&full);
        let hop = || status(302, Some("https://cdn.example.test/blob?sig=SECRET&exp=1"));
        let mut replies: Vec<Reply> = (0..5).map(|_| hop()).collect();
        let mut truncated = ok(&full);
        truncated.body.truncate(1000);
        replies.push(truncated);
        let transport = FakeTransport::new(replies);
        // 途中で切れた → Network。.part と記録は残る（続きから取れる）。
        assert_eq!(run(&spec, &dir, &transport), Err(DownloadError::Network));
        let paths = Paths::new(&dir, &spec);
        assert_eq!(fs::metadata(&paths.part).unwrap().len(), 1000);
        let record = fs::read_to_string(&paths.record).unwrap();
        for leaked in ["SECRET", "sig", "cdn", "https", "example"] {
            assert!(!record.contains(leaked), "record must not contain {leaked}");
        }

        // 6 回目の転送は止める。
        let transport = FakeTransport::new((0..6).map(|_| hop()).collect());
        assert_eq!(
            run(&spec, &temp_dir(), &transport),
            Err(DownloadError::Blocked)
        );
    }

    #[test]
    fn rejects_redirects_outside_the_table() {
        let full = body();
        let spec = spec_for(&full);
        for location in [
            "http://cdn.example.test/blob",
            "https://evil.example.com/blob",
            "https://x.cdn.example.test/blob",
            "https://10.0.0.1/blob",
        ] {
            let transport = FakeTransport::new(vec![status(302, Some(location))]);
            assert_eq!(
                run(&spec, &temp_dir(), &transport),
                Err(DownloadError::Blocked)
            );
        }
    }

    #[test]
    fn stops_on_signal_and_keeps_the_part() {
        let (full, dir) = (body(), temp_dir());
        let spec = spec_for(&full);
        let stop = Arc::new(AtomicBool::new(false));
        let transport = FakeTransport {
            stop_on_read: Some(stop.clone()),
            ..FakeTransport::new(vec![ok(&full)])
        };
        let outcome = download_with(
            &spec,
            &SOURCE,
            &dir,
            &transport,
            &plenty,
            &stop,
            &DownloadProgress::default(),
        );
        assert_eq!(outcome, Ok(DownloadOutcome::Stopped));
        let paths = Paths::new(&dir, &spec);
        assert_eq!(fs::metadata(&paths.part).unwrap().len(), 1000);
        assert!(paths.record.exists() && !paths.complete.exists());

        // 合図が立ったままなら通信を始めない。
        let transport = FakeTransport::new(Vec::new());
        let outcome = download_with(
            &spec,
            &SOURCE,
            &temp_dir(),
            &transport,
            &plenty,
            &stop,
            &DownloadProgress::default(),
        );
        assert_eq!(outcome, Ok(DownloadOutcome::Stopped));
    }

    #[test]
    fn refuses_to_start_without_enough_free_space() {
        let (full, dir) = (body(), temp_dir());
        let spec = spec_for(&full);
        let transport = FakeTransport::new(Vec::new());
        let small = |_: &Path| Some(FREE_SPACE_MARGIN);
        let stop = AtomicBool::new(false);
        let outcome = download_with(
            &spec,
            &SOURCE,
            &dir,
            &transport,
            &small,
            &stop,
            &DownloadProgress::default(),
        );
        assert_eq!(
            outcome,
            Err(DownloadError::InsufficientSpace {
                needed_more: BODY_LEN as u64
            })
        );
        assert!(transport.requests.borrow().is_empty());
    }

    /// 弾いた転送先のログには、ホスト名だけを出す（経路・問い合わせ・署名・利用者情報は出さない）。
    #[test]
    fn rejected_redirect_log_keeps_only_the_host() {
        let current = model_download_url(&SOURCE).unwrap();
        for (location, expected) in [
            (
                "https://eu.cdn.example.test/xet/abc?X-Amz-Signature=secret&exp=1",
                "eu.cdn.example.test",
            ),
            (
                "https://user:pw@other.example.test:8443/p?sig=s",
                "other.example.test",
            ),
            ("http://plain.example.test/path", "plain.example.test"),
            ("https://127.0.0.1/x", "127.0.0.1"),
            ("/relative/path?sig=s", "models.example.test"),
            ("file:///C:/Windows/win.ini", "(none)"),
        ] {
            let host = rejected_redirect_host(&current, location);
            assert_eq!(host, expected, "{location}");
            for secret in ["sig", "Signature", "secret", "pw", "user", "path", "8443"] {
                assert!(!host.contains(secret), "{location}: {host}");
            }
        }
    }

    #[test]
    fn model_without_a_source_is_not_downloaded() {
        let spec = ModelSpec {
            source: None,
            ..spec_for(&body())
        };
        let outcome = download_model(
            &spec,
            &temp_dir(),
            &AtomicBool::new(false),
            &DownloadProgress::default(),
        );
        assert_eq!(outcome, Err(DownloadError::NoSource));
    }

    /// 本物の Hugging Face から `ACTIVE_MODEL`（約 3.1GB）を取る手動確認（CI・ふだんの `cargo test` では動かさない）。
    /// 途中で一度止め（一時停止と同じ合図）、もう一度呼んで続きから取れること、SHA-256 の照合が通って
    /// 完成の名前になることを確かめる。取得先は `YUUKO_MODEL_DL_DIR`（`app_local_data_dir` の代わりの一時の
    /// フォルダ）の下の、アプリと同じ規則のフォルダ（`downloaded_model_dir`）。アプリのデータには触れない。
    /// `YUUKO_MODEL_DL_PAUSE_AT`（バイト。既定 300MB）まで取ったら止める。
    /// `cargo test --manifest-path src-tauri/Cargo.toml real_model_download -- --ignored --nocapture` で動かす。
    #[test]
    #[ignore = "manual: downloads about 3.1GB from Hugging Face into YUUKO_MODEL_DL_DIR"]
    fn real_model_download_pauses_resumes_and_verifies() {
        use crate::infra::local_llm_runtime::{downloaded_model_dir, ACTIVE_MODEL};
        use std::time::Instant;
        let root = PathBuf::from(
            std::env::var_os("YUUKO_MODEL_DL_DIR")
                .expect("set YUUKO_MODEL_DL_DIR to a temp folder"),
        );
        let pause_at: u64 = std::env::var("YUUKO_MODEL_DL_PAUSE_AT")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(300_000_000);
        let dir = downloaded_model_dir(&ACTIVE_MODEL, &root);
        let paths = Paths::new(&dir, &ACTIVE_MODEL);
        assert!(
            !paths.complete.exists(),
            "start without the model: {}",
            dir.display()
        );

        // 1 回目: pause_at を超えたら止める合図を立てる。
        let stop = Arc::new(AtomicBool::new(false));
        let signal = stop.clone();
        let first = DownloadProgress {
            on_progress: Some(Box::new(move |received| {
                if received >= pause_at {
                    signal.store(true, Ordering::Relaxed);
                }
            })),
            ..DownloadProgress::default()
        };
        let started = Instant::now();
        let outcome = download_model(&ACTIVE_MODEL, &dir, &stop, &first);
        let paused = fs::metadata(&paths.part)
            .map(|meta| meta.len())
            .unwrap_or(0);
        eprintln!(
            "first run: {outcome:?} after {:.1}s, part {paused} bytes",
            started.elapsed().as_secs_f32()
        );
        assert_eq!(outcome, Ok(DownloadOutcome::Stopped));
        assert!(paused >= pause_at && paused < ACTIVE_MODEL.size);

        // 2 回目: 続きから（受け取った大きさが一度も paused を下回らない＝最初からやり直していない）。
        let lowest = Arc::new(AtomicU64::new(u64::MAX));
        let seen = lowest.clone();
        let second = DownloadProgress {
            on_progress: Some(Box::new(move |received| {
                seen.fetch_min(received, Ordering::Relaxed);
            })),
            ..DownloadProgress::default()
        };
        let resumed = Instant::now();
        let outcome = download_model(&ACTIVE_MODEL, &dir, &AtomicBool::new(false), &second);
        eprintln!(
            "second run: {outcome:?} after {:.1}s (lowest progress {} bytes)",
            resumed.elapsed().as_secs_f32(),
            lowest.load(Ordering::Relaxed)
        );
        assert_eq!(outcome, Ok(DownloadOutcome::Completed));
        assert_eq!(lowest.load(Ordering::Relaxed), paused);
        assert_eq!(
            fs::metadata(&paths.complete).unwrap().len(),
            ACTIVE_MODEL.size
        );
        assert!(!paths.part.exists() && !paths.record.exists());
        eprintln!(
            "total {:.1}s, model at {}",
            started.elapsed().as_secs_f32(),
            paths.complete.display()
        );
    }

    #[test]
    fn timeouts_cut_only_a_silent_peer() {
        // 全体の時間切れではなく、読み取りごとの上限として 60 秒・接続は 30 秒（設計案 §2.4）。
        assert_eq!(IDLE_TIMEOUT, Duration::from_secs(60));
        assert_eq!(CONNECT_TIMEOUT, Duration::from_secs(30));
    }

    #[test]
    fn real_client_builds_and_free_space_is_readable() {
        client_builder()
            .build()
            .expect("download client must build");
        if cfg!(windows) {
            assert!(available_space(&std::env::temp_dir()).is_some_and(|free| free > 0));
        }
    }
}
