#![windows_subsystem = "windows"]

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use clap::Parser;
use crossbeam_channel::{bounded, Receiver, Sender};
use crc32fast::Hasher as Crc32Hasher;
use eframe::egui;
use egui::{Color32, FontData, FontDefinitions, FontFamily, Frame, Margin, RichText, Rounding, Stroke};
use md5;
use sevenz_rust::{Password, SevenZReader};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

#[cfg(target_os = "windows")]
use winreg::enums::*;
#[cfg(target_os = "windows")]
use winreg::RegKey;

const CHUNK_SIZE: usize = 16 * 1024 * 1024;
const APP_VERSION: &str = "1.0";
const APP_AUTHOR: &str = "kwon0fe";
const APP_EMAIL: &str = "kwon0fe@netsgo.com";
const PAYPAL_URL: &str = "https://paypal.me/kwon0fe";

static EMBED_7Z_EXE: &[u8] = include_bytes!("7z.exe");
static EMBED_7Z_DLL: &[u8] = include_bytes!("7z.dll");
static KAKAOPAY_QR_BYTES: &[u8] = include_bytes!("kakaopay_qr.png");
static EMBED_ICON_ICO: &[u8] = include_bytes!("hash-checker.ico");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Language { Ko, En, Ja, Zh }

impl Language {
    fn from_code(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "en" => Self::En, "ja" => Self::Ja, "zh" => Self::Zh, _ => Self::Ko,
        }
    }
    fn code(self) -> &'static str {
        match self { Self::Ko => "ko", Self::En => "en", Self::Ja => "ja", Self::Zh => "zh" }
    }
    fn date(self) -> &'static str {
        match self {
            Self::Ko => "2026년 11월 24일",
            Self::En => "November 24, 2026",
            Self::Ja => "2026年11月24日",
            Self::Zh => "2026年11月24日",
        }
    }
    fn t(self, k: &str) -> &'static str {
        match (self, k) {
            (Self::Ko, "info") => "정보", (Self::En, "info") => "About", (Self::Ja, "info") => "情報", (Self::Zh, "info") => "关于",
            (Self::Ko, "feedback") => "버그/개선 요청", (Self::En, "feedback") => "Feedback", (Self::Ja, "feedback") => "不具合/要望", (Self::Zh, "feedback") => "反馈/建议",
            (Self::Ko, "fb_title") => "버그 및 개선사항 요청", (Self::En, "fb_title") => "Bug Report & Feature Request", (Self::Ja, "fb_title") => "不具合報告・機能改善の要望", (Self::Zh, "fb_title") => "错误报告与改进建议",
            (Self::Ko, "fb_desc") => "사용 중 발견된 버그나 개선 아이디어가 있다면 아래 이메일로 편하게 보내주세요.",
            (Self::En, "fb_desc") => "If you find any bugs or have suggestions for improvement, please send them to the email address below.",
            (Self::Ja, "fb_desc") => "不具合や機能改善のご要望がございましたら、下記のメールアドレスまでお気軽にお送りください。",
            (Self::Zh, "fb_desc") => "如果您在使用过程中发现任何错误或有改进建议，请发送邮件至以下地址。",
            (Self::Ko, "copy_email") => "이메일 주소 복사", (Self::En, "copy_email") => "Copy Email", (Self::Ja, "copy_email") => "メールアドレスをコピー", (Self::Zh, "copy_email") => "复制邮箱地址",
            (Self::Ko, "copied_done") => "복사 완료!", (Self::En, "copied_done") => "Copied!", (Self::Ja, "copied_done") => "コピー完了！", (Self::Zh, "copied_done") => "已复制！",
            (Self::Ko, "file") => "대상 파일:", (Self::En, "file") => "Target File:", (Self::Ja, "file") => "対象ファイル:", (Self::Zh, "file") => "目标文件:",
            (Self::Ko, "open") => "파일 열기", (Self::En, "open") => "Browse", (Self::Ja, "open") => "参照", (Self::Zh, "open") => "浏览",
            (Self::Ko, "prog") => "진행 상태:", (Self::En, "prog") => "Progress:", (Self::Ja, "prog") => "進行状況:", (Self::Zh, "prog") => "计算进度:",
            (Self::Ko, "ready") => "파일을 선택하거나 화면으로 드래그 앤 드롭하세요.",
            (Self::En, "ready") => "Select a file or drag & drop it onto the window.",
            (Self::Ja, "ready") => "ファイルを選択するか、画面にドラッグ＆ドロップしてください。",
            (Self::Zh, "ready") => "请选择文件或将文件拖放到窗口中。",
            (Self::Ko, "wait_arc") => "압축파일 내부 파일을 계산하고 있으니 잠시 기다려주세요...",
            (Self::En, "wait_arc") => "Calculating archive entries, please wait...",
            (Self::Ja, "wait_arc") => "圧縮ファイル内のファイルを計算中です。しばらくお待ちください...",
            (Self::Zh, "wait_arc") => "正在计算压缩包内部文件，请稍候...",
            (Self::Ko, "done") => "계산 완료 (100%)", (Self::En, "done") => "Completed (100%)", (Self::Ja, "done") => "計算完了 (100%)", (Self::Zh, "done") => "计算完成 (100%)",
            (Self::Ko, "cmp") => "해시 비교:", (Self::En, "cmp") => "Compare Hash:", (Self::Ja, "cmp") => "ハッシュ比較:", (Self::Zh, "cmp") => "哈希比对:",
            (Self::Ko, "hint") => "비교할 해시값을 입력하세요",
            (Self::En, "hint") => "Enter hash value to compare",
            (Self::Ja, "hint") => "比較するハッシュ値を入力してください",
            (Self::Zh, "hint") => "请输入要对比的哈希值",
            (Self::Ko, "paste") => "붙여넣기 (Paste)", (Self::En, "paste") => "Paste", (Self::Ja, "paste") => "貼り付け", (Self::Zh, "paste") => "粘贴",
            (Self::Ko, "clear_in") => "내용 지우기", (Self::En, "clear_in") => "Clear", (Self::Ja, "clear_in") => "クリア", (Self::Zh, "clear_in") => "清空",
            (Self::Ko, "ok") => "일치", (Self::En, "ok") => "Match", (Self::Ja, "ok") => "一致", (Self::Zh, "ok") => "匹配",
            (Self::Ko, "no") => "불일치", (Self::En, "no") => "Mismatch", (Self::Ja, "no") => "不一致", (Self::Zh, "no") => "不匹配",
            (Self::Ko, "arc_list") => "압축 파일 내부 목록", (Self::En, "arc_list") => "Archive Entries", (Self::Ja, "arc_list") => "圧縮ファイル内の項目", (Self::Zh, "arc_list") => "压缩包内部文件列表",
            (Self::Ko, "copy") => "복사 ▾", (Self::En, "copy") => "Copy ▾", (Self::Ja, "copy") => "コピー ▾", (Self::Zh, "copy") => "复制 ▾",
            (Self::Ko, "copy1") => "해시값만 복사", (Self::En, "copy1") => "Copy Hash Only", (Self::Ja, "copy1") => "ハッシュ値のみコピー", (Self::Zh, "copy1") => "仅复制哈希值",
            (Self::Ko, "copy2") => "구분 포함 복사", (Self::En, "copy2") => "Copy with Label", (Self::Ja, "copy2") => "ラベル付きコピー", (Self::Zh, "copy2") => "带名称复制",
            (Self::Ko, "copyall") => "전체 결과 복사", (Self::En, "copyall") => "Copy All", (Self::Ja, "copyall") => "すべてコピー", (Self::Zh, "copyall") => "复制全部",
            (Self::Ko, "clear") => "초기화", (Self::En, "clear") => "Clear", (Self::Ja, "clear") => "初期化", (Self::Zh, "clear") => "清空",
            (Self::Ko, "save") => "결과 저장", (Self::En, "save") => "Save Results", (Self::Ja, "save") => "結果を保存", (Self::Zh, "save") => "保存结果",
            (Self::Ko, "reg") => "우클릭 메뉴 등록", (Self::En, "reg") => "Register Context Menu", (Self::Ja, "reg") => "右クリックメニュー登録", (Self::Zh, "reg") => "注册右键菜单",
            (Self::Ko, "unreg") => "우클릭 메뉴 해제", (Self::En, "unreg") => "Unregister Context Menu", (Self::Ja, "unreg") => "右クリックメニュー解除", (Self::Zh, "unreg") => "取消右键菜单",
            (Self::Ko, "reg_ok") => "우클릭 메뉴가 등록되었습니다.", (Self::En, "reg_ok") => "Context menu registered.", (Self::Ja, "reg_ok") => "右クリックメニューを登録しました。", (Self::Zh, "reg_ok") => "已注册右键菜单。",
            (Self::Ko, "unreg_ok") => "우클릭 메뉴가 해제되었습니다.", (Self::En, "unreg_ok") => "Context menu unregistered.", (Self::Ja, "unreg_ok") => "右クリックメニューを解除しました。", (Self::Zh, "unreg_ok") => "已取消右键菜单。",
            (Self::Ko, "saved_ok") => "결과 파일이 저장되었습니다.", (Self::En, "saved_ok") => "Results saved.", (Self::Ja, "saved_ok") => "結果を保存しました。", (Self::Zh, "saved_ok") => "结果已保存。",
            (Self::Ko, "about_t") => "프로그램 정보", (Self::En, "about_t") => "About Hash Checker", (Self::Ja, "about_t") => "プログラム情報", (Self::Zh, "about_t") => "关于程序",
            (Self::Ko, "sub") => "대용량 및 압축 파일 무결성 해시 검증 도구",
            (Self::En, "sub") => "High-Speed Large File & Archive Hash Verification Tool",
            (Self::Ja, "sub") => "大容量・圧縮ファイル整合性ハッシュ検証ツール",
            (Self::Zh, "sub") => "大文件及压缩包完整性哈希校验工具",
            (Self::Ko, "author_l") => "만든이", (Self::En, "author_l") => "Author", (Self::Ja, "author_l") => "作成者", (Self::Zh, "author_l") => "作者",
            (Self::Ko, "date_l") => "배포일", (Self::En, "date_l") => "Release Date", (Self::Ja, "date_l") => "配布日", (Self::Zh, "date_l") => "发布日期",
            (Self::Ko, "email_l") => "이메일", (Self::En, "email_l") => "Email", (Self::Ja, "email_l") => "メール", (Self::Zh, "email_l") => "电子邮箱",
            (Self::Ko, "lic") => "라이선스: 프리웨어 (Freeware - 개인 및 기업 무료 사용)",
            (Self::En, "lic") => "License: Freeware (Free for personal and commercial use)",
            (Self::Ja, "lic") => "ライセンス: フリーウェア (Freeware - 個人・法人無料)",
            (Self::Zh, "lic") => "许可证: 免费软件 (Freeware - 个人及商业免费使用)",
            (Self::Ko, "close") => "닫기", (Self::En, "close") => "Close", (Self::Ja, "close") => "閉じる", (Self::Zh, "close") => "关闭",
            (Self::Ko, "don_msg") => "이 프로그램이 도움이 되셨다면 개발자에게 커피 한 잔을 선물해 주세요. (선택 사항이며 모든 기능은 무료입니다.)",
            (Self::En, "don_msg") => "If you find this tool helpful, consider buying the developer a coffee. (Completely optional — all features are free!)",
            (Self::Ja, "don_msg") => "このツールがお役に立ちましたら、開発者にコーヒー1杯をご馳走していただけると嬉しいです。（任意・全機能無料）",
            (Self::Zh, "don_msg") => "如果这款工具对您有所帮助，欢迎请开发者喝杯咖啡。（完全自愿，所有功能均免费使用）",
            (Self::Ko, "don_kp") => "카카오페이 후원",
            (Self::Ko, "don_pp") => "PayPal 후원", (Self::En, "don_pp") => "Donate via PayPal", (Self::Ja, "don_pp") => "PayPalで応援する", (Self::Zh, "don_pp") => "通过 PayPal 赞助",
            _ => "",
        }
    }
}
#[derive(Parser, Debug)]
#[command(name = "hash-checker", version = APP_VERSION)]
struct Args {
    #[arg(value_name = "PATH")]
    path: Option<PathBuf>,
    #[arg(long, hide = true)]
    register_context_menu: bool,
    #[arg(long, hide = true)]
    unregister_context_menu: bool,
    #[arg(long, hide = true)]
    gui: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AppConfig { crc32: bool, md5: bool, sha1: bool, sha256: bool, lang: Language }

impl Default for AppConfig {
    fn default() -> Self { Self { crc32: true, md5: true, sha1: true, sha256: true, lang: Language::Ko } }
}

fn get_config_path() -> Option<PathBuf> {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("hash-checker.ini")))
}

fn load_app_config() -> AppConfig {
    let mut cfg = AppConfig::default();
    if let Some(path) = get_config_path() {
        if let Ok(content) = std::fs::read_to_string(path) {
            for line in content.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    let val = v.trim() == "true" || v.trim() == "1";
                    match k.trim() {
                        "crc32" => cfg.crc32 = val,
                        "md5" => cfg.md5 = val,
                        "sha1" => cfg.sha1 = val,
                        "sha256" => cfg.sha256 = val,
                        "lang" => cfg.lang = Language::from_code(v),
                        _ => {}
                    }
                }
            }
        }
    }
    cfg
}

fn save_app_config(cfg: AppConfig) {
    if let Some(path) = get_config_path() {
        let s = format!("crc32={}\nmd5={}\nsha1={}\nsha256={}\nlang={}\n", cfg.crc32, cfg.md5, cfg.sha1, cfg.sha256, cfg.lang.code());
        let _ = std::fs::write(path, s);
    }
}

#[derive(Debug, Clone)]
enum HashTask { File(PathBuf, AppConfig), Archive(PathBuf, AppConfig) }

#[derive(Debug, Clone)]
struct FileHashResult { path: String, size: u64, crc32: Option<String>, md5: Option<String>, sha1: Option<String>, sha256: Option<String> }

#[derive(Debug, Clone)]
enum ComputeMessage { Progress(f32, String), FileResult(FileHashResult), ArchiveEntryResult(FileHashResult), Finished, Error(String) }

fn format_size(bytes: u64) -> String {
    let s = bytes.to_string();
    let mut c = String::new();
    for (i, ch) in s.chars().rev().enumerate() { if i > 0 && i % 3 == 0 { c.push(','); } c.push(ch); }
    let fb: String = c.chars().rev().collect();
    if bytes >= 1024 * 1024 * 1024 { format!("{:.2} GB ({} bytes)", bytes as f64 / 1073741824.0, fb) }
    else if bytes >= 1024 * 1024 { format!("{:.2} MB ({} bytes)", bytes as f64 / 1048576.0, fb) }
    else if bytes >= 1024 { format!("{:.2} KB ({} bytes)", bytes as f64 / 1024.0, fb) }
    else { format!("{} bytes", fb) }
}

fn format_elapsed(d: Duration) -> String {
    let secs = d.as_secs();
    format!("{:02}:{:02}.{}", secs / 60, secs % 60, d.subsec_millis() / 100)
}

fn ensure_embedded_7z() -> Option<PathBuf> {
    let dir = std::env::temp_dir().join("hc_7z_embedded_v1");
    let exe_path = dir.join("7z.exe");
    let dll_path = dir.join("7z.dll");
    if exe_path.exists() && dll_path.exists() { return Some(exe_path); }
    let _ = std::fs::create_dir_all(&dir);
    if std::fs::write(&exe_path, EMBED_7Z_EXE).is_ok() && std::fs::write(&dll_path, EMBED_7Z_DLL).is_ok() {
        return Some(exe_path);
    }
    None
}

fn find_7z_exe() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in ["7z.exe", "7za.exe"] {
                let p = dir.join(name);
                if p.exists() { return Some(p); }
            }
        }
    }
    ensure_embedded_7z()
}

fn compute_stream(reader: &mut dyn Read, opts: AppConfig, total: u64, tx: &Sender<ComputeMessage>, label: &str, p0: f32, span: f32, buf: &mut [u8]) -> (Option<String>, Option<String>, Option<String>, Option<String>) {
    let (mut h_crc, mut h_md5, mut h_sha1, mut h_sha256) = (Crc32Hasher::new(), md5::Context::new(), Sha1::new(), Sha256::new());
    let mut done: u64 = 0;
    let mut last_ui = Instant::now();
    loop {
        let mut filled = 0;
        while filled < buf.len() {
            match reader.read(&mut buf[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(_) => break,
            }
        }
        if filled == 0 { break; }
        let d = &buf[..filled];
        if opts.crc32 { h_crc.update(d); }
        if opts.md5 { h_md5.consume(d); }
        if opts.sha1 { h_sha1.update(d); }
        if opts.sha256 { h_sha256.update(d); }
        done += filled as u64;
        if total > 0 && last_ui.elapsed() >= Duration::from_millis(200) {
            let p = p0 + (done as f32 / total as f32).min(1.0) * span;
            let _ = tx.send(ComputeMessage::Progress(p, label.to_string()));
            last_ui = Instant::now();
        }
    }
    (
        if opts.crc32 { Some(format!("{:08x}", h_crc.finalize())) } else { None },
        if opts.md5 { Some(format!("{:x}", h_md5.compute())) } else { None },
        if opts.sha1 { Some(format!("{:x}", h_sha1.finalize())) } else { None },
        if opts.sha256 { Some(format!("{:x}", h_sha256.finalize())) } else { None },
    )
}

fn run_7z_cli(exe: &PathBuf, path: &PathBuf, opts: AppConfig, tx: &Sender<ComputeMessage>, buf: &mut [u8]) -> Result<(), String> {
    let mut cmd = Command::new(exe);
    cmd.args(["l", "-slt", "-ba", "-sccUTF-8"]).arg(path).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(target_os = "windows")] { cmd.creation_flags(0x08000000); }
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() { return Err("7z list err".into()); }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut entries: Vec<(String, u64)> = Vec::new();
    let (mut cur_p, mut cur_s, mut is_dir) = (String::new(), 0u64, false);
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() {
            if !cur_p.is_empty() && !is_dir { entries.push((cur_p.clone(), cur_s)); }
            cur_p.clear(); cur_s = 0; is_dir = false;
        } else if let Some(v) = l.strip_prefix("Path = ") { cur_p = v.to_string(); }
        else if let Some(v) = l.strip_prefix("Size = ") { cur_s = v.parse().unwrap_or(0); }
        else if let Some(v) = l.strip_prefix("Folder = ") { is_dir = v == "+"; }
    }
    if !cur_p.is_empty() && !is_dir { entries.push((cur_p, cur_s)); }
    let tot = entries.len().max(1);
    if entries.len() <= 1 {
        for (i, (ename, esz)) in entries.into_iter().enumerate() {
            let p0 = (i as f32 / tot as f32) * 0.98_f32;
            let sp = (1.0_f32 / tot as f32) * 0.98_f32;
            let lbl = format!("[7-Zip 고속] {} ({}/{}) - {}", opts.lang.t("wait_arc"), i + 1, tot, ename);
            let _ = tx.send(ComputeMessage::Progress(p0, lbl.clone()));
            let mut xcmd = Command::new(exe);
            xcmd.args(["x", "-so", "-sccUTF-8"]).arg(path).arg(&ename).stdout(Stdio::piped()).stderr(Stdio::null());
            #[cfg(target_os = "windows")] { xcmd.creation_flags(0x08000000); }
            let mut child = xcmd.spawn().map_err(|e| e.to_string())?;
            if let Some(mut stdout) = child.stdout.take() {
                let (crc32, md5, sha1, sha256) = compute_stream(&mut stdout, opts, esz, tx, &lbl, p0, sp, buf);
                let _ = tx.send(ComputeMessage::ArchiveEntryResult(FileHashResult { path: ename, size: esz, crc32, md5, sha1, sha256 }));
            }
            let _ = child.wait();
        }
    } else {
        let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
        let tmp_dir = std::env::temp_dir().join(format!("hc_7z_batch_{}_{}", std::process::id(), ts));
        let _ = std::fs::create_dir_all(&tmp_dir);
        let _ = tx.send(ComputeMessage::Progress(0.05_f32, format!("[7-Zip 고속] {}", opts.lang.t("wait_arc"))));
        let mut xcmd = Command::new(exe);
        xcmd.args(["x", "-y", "-sccUTF-8"]).arg(format!("-o{}", tmp_dir.display())).arg(path).stdout(Stdio::null()).stderr(Stdio::null());
        #[cfg(target_os = "windows")] { xcmd.creation_flags(0x08000000); }
        let st = xcmd.status().map_err(|e| { let _ = std::fs::remove_dir_all(&tmp_dir); e.to_string() })?;
        if !st.success() { let _ = std::fs::remove_dir_all(&tmp_dir); return Err("7z batch err".into()); }
        for (i, (ename, esz)) in entries.into_iter().enumerate() {
            let p0 = 0.30_f32 + (i as f32 / tot as f32) * 0.68_f32;
            let sp = (1.0_f32 / tot as f32) * 0.68_f32;
            let lbl = format!("[7-Zip 고속] {} ({}/{}) - {}", opts.lang.t("wait_arc"), i + 1, tot, ename);
            let _ = tx.send(ComputeMessage::Progress(p0, lbl.clone()));
            let fpath = tmp_dir.join(&ename);
            if let Ok(f) = File::open(&fpath) {
                let mut br = BufReader::with_capacity(CHUNK_SIZE, f);
                let (crc32, md5, sha1, sha256) = compute_stream(&mut br, opts, esz, tx, &lbl, p0, sp, buf);
                let _ = tx.send(ComputeMessage::ArchiveEntryResult(FileHashResult { path: ename, size: esz, crc32, md5, sha1, sha256 }));
            }
        }
        let _ = std::fs::remove_dir_all(&tmp_dir);
    }
    Ok(())
}

fn compute_worker(rx: Receiver<HashTask>, tx: Sender<ComputeMessage>) {
    let mut buf = vec![0u8; CHUNK_SIZE];
    while let Ok(task) = rx.recv() {
        match task {
            HashTask::File(path, opts) => {
                let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                let res = File::open(&path).map_err(|e| e.to_string()).map(|f| {
                    let sz = f.metadata().map(|m| m.len()).unwrap_or(0);
                    let mut r = BufReader::with_capacity(CHUNK_SIZE, f);
                    let (crc32, md5, sha1, sha256) = compute_stream(&mut r, opts, sz, &tx, &name, 0.0, 0.99_f32, &mut buf);
                    FileHashResult { path: name, size: sz, crc32, md5, sha1, sha256 }
                });
                match res {
                    Ok(r) => { let _ = tx.send(ComputeMessage::FileResult(r)); let _ = tx.send(ComputeMessage::Progress(1.0, opts.lang.t("done").to_string())); }
                    Err(e) => { let _ = tx.send(ComputeMessage::Error(e)); }
                }
            }
            HashTask::Archive(path, opts) => {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                let cli_res = if let Some(exe7z) = find_7z_exe() { run_7z_cli(&exe7z, &path, opts, &tx, &mut buf) } else { Err("no_7z".into()) };
                let res = if cli_res.is_ok() { Ok(()) } else if ext == "zip" {
                    File::open(&path).map_err(|e| e.to_string()).and_then(|f| ZipArchive::new(f).map_err(|e| e.to_string())).and_then(|mut arc| {
                        let tot = arc.len();
                        for i in 0..tot {
                            let mut ent = arc.by_index(i).map_err(|e| e.to_string())?;
                            if ent.is_dir() { continue; }
                            let ename = ent.name().to_string();
                            let esz = ent.size();
                            let p0 = (i as f32 / tot.max(1) as f32) * 0.98_f32;
                            let sp = (1.0_f32 / tot.max(1) as f32) * 0.98_f32;
                            let lbl = format!("{} ({}/{}) - {}", opts.lang.t("wait_arc"), i + 1, tot, ename);
                            let _ = tx.send(ComputeMessage::Progress(p0, lbl.clone()));
                            let (crc32, md5, sha1, sha256) = compute_stream(&mut ent, opts, esz, &tx, &lbl, p0, sp, &mut buf);
let _ = tx.send(ComputeMessage::ArchiveEntryResult(FileHashResult { path: ename, size: esz, crc32, md5, sha1, sha256 }));
}
Ok(())
})
} else {
File::open(&path).map_err(|e| e.to_string()).and_then(|f| {
let flen = f.metadata().map(|m| m.len()).unwrap_or(0);
let mut br = BufReader::with_capacity(CHUNK_SIZE, f);
let mut rdr = SevenZReader::new(&mut br, flen, Password::empty()).map_err(|e| e.to_string())?;
let mut idx = 0usize;
let mut done_unpacked: u64 = 0;
rdr.for_each_entries(|ent, erdr| {
if ent.is_directory() || !ent.has_stream() { return Ok(true); }
idx += 1;
let ename = ent.name().to_string();
let esz = ent.size();
let est_total = (done_unpacked + esz).max(flen).max(1);
let p0 = (done_unpacked as f32 / est_total as f32) * 0.96_f32;
let sp = ((esz as f32 / est_total as f32) * 0.96_f32).min(0.98_f32 - p0).max(0.01_f32);
let lbl = format!("{} (#{}) - {}", opts.lang.t("wait_arc"), idx, ename);
let _ = tx.send(ComputeMessage::Progress(p0, lbl.clone()));
let (crc32, md5, sha1, sha256) = compute_stream(erdr, opts, esz, &tx, &lbl, p0, sp, &mut buf);
let _ = tx.send(ComputeMessage::ArchiveEntryResult(FileHashResult { path: ename, size: esz, crc32, md5, sha1, sha256 }));
done_unpacked += esz;
Ok(true)
}).map_err(|e| e.to_string())
})
};
match res {
Ok(()) => { let _ = tx.send(ComputeMessage::Progress(1.0, opts.lang.t("done").to_string())); }
Err(e) => { let _ = tx.send(ComputeMessage::Error(e)); }
}
}
}
let _ = tx.send(ComputeMessage::Finished);
}
}
#[cfg(target_os = "windows")]
fn register_context_menu() -> Result<(), Box<dyn std::error::Error>> {
    let exe = std::env::current_exe()?.to_string_lossy().to_string();
    let local_app = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().to_string());
    let helper_dir = PathBuf::from(local_app).join("HashChecker");
    let _ = std::fs::create_dir_all(&helper_dir);
    let vbs_path = helper_dir.join("launch.vbs");

    let exe_vbs = exe.replace('"', "\"\"");
    let vbs_tpl = r#"Set fso = CreateObject("Scripting.FileSystemObject")
Set sh = CreateObject("WScript.Shell")
exePath = "__EXE__"
If fso.FileExists(exePath) Then
    If WScript.Arguments.Count > 0 Then
        sh.Run """" & exePath & """ --gui """ & WScript.Arguments(0) & """", 1, False
    Else
        sh.Run """" & exePath & """", 1, False
    End If
Else
    lang = ""
    iniPath = fso.GetParentFolderName(exePath) & "\hash-checker.ini"
    If fso.FileExists(iniPath) Then
        On Error Resume Next
        Set ts = fso.OpenTextFile(iniPath, 1, False, 0)
        Do While Not ts.AtEndOfStream
            ln = Trim(ts.ReadLine)
            If Left(ln, 5) = "lang=" Then lang = LCase(Trim(Mid(ln, 6)))
        Loop
        ts.Close
        On Error GoTo 0
    End If
    If lang = "" Then
        loc = GetLocale()
        If loc = 1042 Then
            lang = "ko"
        ElseIf loc = 1041 Then
            lang = "ja"
        ElseIf loc = 2052 Or loc = 1028 Then
            lang = "zh"
        Else
            lang = "en"
        End If
    End If
    If lang = "ko" Then
        msg = "Hash Checker 실행 파일의 위치가 변경되었거나 삭제되었습니다." & vbCrLf & "우클릭 메뉴가 자동으로 해제됩니다. (다시 사용하려면 새 위치에서 [우클릭 메뉴 등록]을 눌러주세요.)"
    ElseIf lang = "ja" Then
        msg = "Hash Checker の実行ファイルが移動または削除されました。" & vbCrLf & "右クリックメニューは自動的に解除されます。（再度使用する場合は新しい場所で登録してください。）"
    ElseIf lang = "zh" Then
        msg = "Hash Checker 执行文件已被移动或删除。" & vbCrLf & "右键菜单将自动取消。（如需再次使用，请在新位置重新注册。）"
    Else
        msg = "Hash Checker executable was moved, renamed, or deleted." & vbCrLf & "The context menu has been automatically removed. (Please re-register from the new location.)"
    End If
    MsgBox msg, 48, "Hash Checker"
    On Error Resume Next
    sh.Run "reg.exe delete ""HKCU\Software\Classes\*\shell\HashChecker"" /f", 0, True
    fso.DeleteFile WScript.ScriptFullName, True
End If
"#;
    let vbs_content = vbs_tpl.replace("__EXE__", &exe_vbs).replace("\n", "\r\n");
    let mut utf16_bytes = vec![0xFFu8, 0xFEu8];
    for u in vbs_content.encode_utf16() {
        utf16_bytes.extend_from_slice(&u.to_le_bytes());
    }
    std::fs::write(&vbs_path, utf16_bytes)?;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let _ = hkcu.delete_subkey_all(r"Software\Classes\*\shell\HashChecker");
    let _ = RegKey::predef(HKEY_CLASSES_ROOT).delete_subkey_all(r"*\shell\HashChecker");
    let (key, _) = hkcu.create_subkey(r"Software\Classes\*\shell\HashChecker")?;
    key.set_value("", &"Hash Checker")?;
    key.set_value("Icon", &format!("\"{}\",0", exe))?;
    let (cmd, _) = hkcu.create_subkey(r"Software\Classes\*\shell\HashChecker\command")?;
    cmd.set_value("", &format!("wscript.exe \"{}\" \"%1\"", vbs_path.to_string_lossy()))?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn unregister_context_menu() -> Result<(), Box<dyn std::error::Error>> {
let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(r"Software\Classes*\shell\HashChecker");
let _ = RegKey::predef(HKEY_CLASSES_ROOT).delete_subkey_all(r"*\shell\HashChecker");
if let Ok(local_app) = std::env::var("LOCALAPPDATA") {
let _ = std::fs::remove_dir_all(PathBuf::from(local_app).join("HashChecker"));
}
Ok(())
}
#[cfg(target_os = "windows")]
fn sync_context_menu_if_registered() {
if RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Classes*\shell\HashChecker").is_ok() {
let _ = register_context_menu();
}
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoticeKind { None, RegOk, UnregOk, SavedOk }

struct HashCheckerApp {
    file_path: String, main_size: u64, is_archive: bool,
    show_crc32: bool, show_md5: bool, show_sha1: bool, show_sha256: bool, lang: Language,
    crc32: String, md5: String, sha1: String, sha256: String,
    tx: Sender<HashTask>, rx: Receiver<ComputeMessage>,
    computing: bool, progress: f32, status: String,
    start_time: Option<Instant>, elapsed: Duration,
    auto_run: bool, archive_list: Arc<Mutex<Vec<FileHashResult>>>,
    verify_in: String, show_about: bool, show_feedback: bool, fb_copied: bool,
    show_qr: bool, qr_tex: Option<egui::TextureHandle>,
    notice_kind: NoticeKind, row_notice: String, themed: bool,
}

impl Default for HashCheckerApp {
    fn default() -> Self {
        let (ttx, trx) = bounded(10);
        let (mtx, mrx) = bounded(200);
        let arc = Arc::new(Mutex::new(Vec::new()));
        let cfg = load_app_config();
        thread::spawn(move || compute_worker(trx, mtx));
        Self {
            file_path: String::new(), main_size: 0, is_archive: false,
            show_crc32: cfg.crc32, show_md5: cfg.md5, show_sha1: cfg.sha1, show_sha256: cfg.sha256, lang: cfg.lang,
            crc32: String::new(), md5: String::new(), sha1: String::new(), sha256: String::new(),
            tx: ttx, rx: mrx, computing: false, progress: 0.0, status: String::new(),
            start_time: None, elapsed: Duration::ZERO,
            auto_run: false, archive_list: arc, verify_in: String::new(),
            show_about: false, show_feedback: false, fb_copied: false,
            show_qr: false, qr_tex: None, notice_kind: NoticeKind::None, row_notice: String::new(), themed: false,
        }
    }
}

fn truncate_front_to_fit(ui: &egui::Ui, text: &str, font_id: egui::FontId, max_w: f32) -> String {
    let full_w = ui.fonts(|f| f.layout_no_wrap(text.to_string(), font_id.clone(), Color32::BLACK).size().x);
    if full_w <= max_w {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut lo = 0usize;
    let mut hi = chars.len();
    let mut best = String::from("...");
    while lo < hi {
        let mid = (lo + hi) / 2;
        let tail: String = chars[mid..].iter().collect();
        let cand = format!("...{}", tail);
        let w = ui.fonts(|f| f.layout_no_wrap(cand.clone(), font_id.clone(), Color32::BLACK).size().x);
        if w <= max_w {
            best = cand;
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    best
}

fn draw_hash_row(ui: &mut egui::Ui, id: &str, algo: &str, val: &str, all: &str, notice: &mut String, lang: Language) {
    let bg = match algo {
        "CRC32" => Color32::from_rgb(37, 99, 235),
        "MD5" => Color32::from_rgb(13, 148, 136),
        "SHA1" => Color32::from_rgb(124, 58, 237),
        _ => Color32::from_rgb(217, 119, 6),
    };
    ui.horizontal(|ui| {
        Frame::none().fill(bg).rounding(Rounding::same(4.0)).inner_margin(Margin::symmetric(6.0, 3.0)).show(ui, |ui| {
            ui.set_width(50.0);
            ui.centered_and_justified(|ui| { ui.label(RichText::new(algo).color(Color32::WHITE).strong().size(11.5)); });
        });
        let box_w = (ui.available_width() - 72.0).max(120.0);
        Frame::none().fill(Color32::from_rgb(241, 245, 249)).stroke(Stroke::new(1.0_f32, Color32::from_rgb(203, 213, 225))).rounding(Rounding::same(4.0)).inner_margin(Margin::symmetric(8.0, 4.0)).show(ui, |ui| {
            ui.set_width(box_w - 16.0);
            ui.set_min_height(18.0);
            ui.add(egui::Label::new(RichText::new(if val.is_empty() { " " } else { val }).monospace().size(12.5).color(Color32::from_rgb(15, 23, 42))).selectable(true));
        });
        let bid = ui.make_persistent_id(id);
        let btn = ui.add_enabled(!val.is_empty(), egui::Button::new(RichText::new(lang.t("copy")).size(12.0)).min_size(egui::vec2(64.0, 26.0)));
        if btn.clicked() { ui.memory_mut(|m| m.toggle_popup(bid)); }
        egui::popup::popup_below_widget(ui, bid, &btn, egui::PopupCloseBehavior::CloseOnClickOutside, |ui: &mut egui::Ui| {
            ui.set_min_width(175.0);
            if ui.button(lang.t("copy1")).clicked() { ui.output_mut(|o| o.copied_text = val.to_string()); *notice = format!("{} OK", algo); ui.memory_mut(|m| m.close_popup()); }
            if ui.button(lang.t("copy2")).clicked() { ui.output_mut(|o| o.copied_text = format!("{}: {}", algo, val)); *notice = format!("{} OK", algo); ui.memory_mut(|m| m.close_popup()); }
            if !all.is_empty() {
                ui.separator();
                if ui.button(lang.t("copyall")).clicked() { ui.output_mut(|o| o.copied_text = all.to_string()); *notice = "ALL OK".to_string(); ui.memory_mut(|m| m.close_popup()); }
            }
        });
    });
}

impl eframe::App for HashCheckerApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        save_app_config(AppConfig { crc32: self.show_crc32, md5: self.show_md5, sha1: self.show_sha1, sha256: self.show_sha256, lang: self.lang });
    }
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.themed {
            let mut v = egui::Visuals::light();
            v.panel_fill = Color32::from_rgb(241, 245, 249);
            v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(203, 213, 225));
            ctx.set_visuals(v);
            self.themed = true;
            ctx.request_repaint();
        }
        if self.computing {
            if let Some(st) = self.start_time { self.elapsed = st.elapsed(); }
            ctx.request_repaint();
        }
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                ComputeMessage::Progress(p, s) => { self.progress = p; self.status = s; }
                ComputeMessage::FileResult(r) => { self.main_size = r.size; self.crc32 = r.crc32.unwrap_or_default(); self.md5 = r.md5.unwrap_or_default(); self.sha1 = r.sha1.unwrap_or_default(); self.sha256 = r.sha256.unwrap_or_default(); }
                ComputeMessage::ArchiveEntryResult(r) => { if let Ok(mut l) = self.archive_list.lock() { l.push(r); } }
                ComputeMessage::Finished => { if let Some(st) = self.start_time { self.elapsed = st.elapsed(); } self.computing = false; }
                ComputeMessage::Error(e) => { self.status = format!("Error: {}", e); self.computing = false; }
            }
        }
        if !self.auto_run && !self.file_path.is_empty() && !self.computing {
            self.auto_run = true;
            self.start_job();
        }
        ctx.input(|i| {
            if !self.computing && !i.raw.dropped_files.is_empty() {
                if let Some(p) = &i.raw.dropped_files.first().and_then(|f| f.path.as_ref()) {
                    self.file_path = p.display().to_string();
                    self.reset_results_only();
                    self.auto_run = false;
                }
            }
        });
        let lang = self.lang;
        egui::TopBottomPanel::top("hdr").frame(Frame::none().fill(Color32::from_rgb(30, 58, 138)).inner_margin(Margin::symmetric(14.0, 10.0))).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Hash Checker").color(Color32::WHITE).size(18.0).strong());
                ui.label(RichText::new(format!("v{}", APP_VERSION)).color(Color32::from_rgb(191, 219, 254)).size(13.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(RichText::new(lang.t("info")).color(Color32::from_rgb(30, 58, 138)).strong()).fill(Color32::from_rgb(219, 234, 254))).clicked() {
                        self.show_about = !self.show_about;
                    }
                    ui.add_space(4.0);
                    if ui.add(egui::Button::new(RichText::new(lang.t("feedback")).color(Color32::WHITE).strong().size(11.5)).fill(Color32::from_rgb(13, 148, 136))).clicked() {
                        self.show_feedback = true;
                        self.fb_copied = false;
                    }
                    ui.add_space(6.0);
                    for (lg, lbl) in [(Language::Zh, "CN 中文"), (Language::Ja, "JP 日本語"), (Language::En, "US English"), (Language::Ko, "KR 한국어")] {
                        let sel = self.lang == lg;
                        let b = egui::Button::new(RichText::new(lbl).size(11.5).strong().color(if sel { Color32::from_rgb(30, 58, 138) } else { Color32::WHITE }))
                            .fill(if sel { Color32::from_rgb(253, 224, 71) } else { Color32::from_rgb(37, 99, 235) });
                        if ui.add(b).clicked() {
                            self.lang = lg;
                            save_app_config(AppConfig { crc32: self.show_crc32, md5: self.show_md5, sha1: self.show_sha1, sha256: self.show_sha256, lang: self.lang });
                        }
                    }
                });
            });
        });
        egui::TopBottomPanel::bottom("btm").frame(Frame::none().fill(Color32::WHITE).stroke(Stroke::new(1.0_f32, Color32::from_rgb(203, 213, 225))).inner_margin(Margin::symmetric(14.0, 8.0))).show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(!self.computing, egui::Button::new(lang.t("clear"))).clicked() { self.full_reset(); }
                if ui.add_enabled(!self.file_path.is_empty() && !self.computing, egui::Button::new(RichText::new(lang.t("save")).color(Color32::WHITE).strong()).fill(Color32::from_rgb(37, 99, 235))).clicked() { self.save_txt(); }
                #[cfg(target_os = "windows")]
                if ui.button(lang.t("reg")).clicked() { if register_context_menu().is_ok() { self.notice_kind = NoticeKind::RegOk; self.row_notice.clear(); } }
                #[cfg(target_os = "windows")]
                if ui.button(lang.t("unreg")).clicked() { if unregister_context_menu().is_ok() { self.notice_kind = NoticeKind::UnregOk; self.row_notice.clear(); } }
                let n_msg = match self.notice_kind {
                    NoticeKind::RegOk => lang.t("reg_ok"),
                    NoticeKind::UnregOk => lang.t("unreg_ok"),
                    NoticeKind::SavedOk => lang.t("saved_ok"),
                    NoticeKind::None => "",
                };
                if !n_msg.is_empty() { ui.label(RichText::new(n_msg).color(Color32::from_rgb(22, 163, 74)).strong()); }
            });
        });
        egui::CentralPanel::default().frame(Frame::none().fill(Color32::from_rgb(241, 245, 249)).inner_margin(Margin::same(12.0))).show(ctx, |ui| {
Frame::none().fill(Color32::WHITE).stroke(Stroke::new(1.0_f32, Color32::from_rgb(203, 213, 225))).rounding(Rounding::same(8.0)).inner_margin(Margin::same(12.0)).show(ui, |ui| {
ui.horizontal(|ui| {
ui.label(RichText::new(lang.t("file")).strong());
if ui.add_enabled(!self.computing, egui::TextEdit::singleline(&mut self.file_path).desired_width((ui.available_width() - 95.0).max(200.0))).changed() { self.reset_results_only(); self.auto_run = false; }
if ui.add_enabled(!self.computing, egui::Button::new(RichText::new(lang.t("open")).color(Color32::WHITE).strong()).fill(Color32::from_rgb(37, 99, 235))).clicked() {
if let Some(p) = rfd::FileDialog::new().pick_file() { self.file_path = p.display().to_string(); self.reset_results_only(); self.auto_run = false; }
}
});
ui.add_space(6.0);
ui.horizontal(|ui| {
let mut ch = false;
ch |= ui.checkbox(&mut self.show_crc32, "CRC32").changed();
ch |= ui.checkbox(&mut self.show_md5, "MD5").changed();
ch |= ui.checkbox(&mut self.show_sha1, "SHA1").changed();
ch |= ui.checkbox(&mut self.show_sha256, "SHA256").changed();
if ch { save_app_config(AppConfig { crc32: self.show_crc32, md5: self.show_md5, sha1: self.show_sha1, sha256: self.show_sha256, lang: self.lang }); }
if self.main_size > 0 && !self.is_archive { ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { ui.label(RichText::new(format_size(self.main_size)).color(Color32::from_rgb(71, 85, 105)).size(12.0)); }); }
});
ui.add_space(6.0);
ui.horizontal(|ui| {
ui.label(RichText::new(lang.t("prog")).strong());
ui.label(RichText::new(format!("[{}]", format_elapsed(self.elapsed))).monospace().strong().color(Color32::from_rgb(30, 58, 138)));
ui.add(egui::ProgressBar::new(self.progress).fill(Color32::from_rgb(37, 99, 235)).show_percentage().animate(self.computing));
});
let st = if self.status.is_empty() { lang.t("ready") } else if !self.computing && self.progress >= 1.0 { lang.t("done") } else { &self.status };
ui.label(RichText::new(st).color(if self.computing { Color32::from_rgb(29, 78, 216) } else { Color32::from_rgb(71, 85, 105) }).size(12.0));
});
if !self.file_path.is_empty() {
                ui.add_space(8.0);
                let all = [
                    if self.show_crc32 && !self.crc32.is_empty() { format!("CRC32:  {}", self.crc32) } else { String::new() },
                    if self.show_md5 && !self.md5.is_empty() { format!("MD5:    {}", self.md5) } else { String::new() },
                    if self.show_sha1 && !self.sha1.is_empty() { format!("SHA1:   {}", self.sha1) } else { String::new() },
                    if self.show_sha256 && !self.sha256.is_empty() { format!("SHA256: {}", self.sha256) } else { String::new() },
                ].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n");
                Frame::none().fill(Color32::WHITE).stroke(Stroke::new(1.0_f32, Color32::from_rgb(203, 213, 225))).rounding(Rounding::same(8.0)).inner_margin(Margin::same(12.0)).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    if !self.is_archive {
                        if self.show_crc32 { draw_hash_row(ui, "m_crc", "CRC32", &self.crc32, &all, &mut self.row_notice, lang); }
                        if self.show_md5 { draw_hash_row(ui, "m_md5", "MD5", &self.md5, &all, &mut self.row_notice, lang); }
                        if self.show_sha1 { draw_hash_row(ui, "m_sha1", "SHA1", &self.sha1, &all, &mut self.row_notice, lang); }
                        if self.show_sha256 { draw_hash_row(ui, "m_sha256", "SHA256", &self.sha256, &all, &mut self.row_notice, lang); }
                        ui.add_space(6.0); ui.separator(); ui.add_space(4.0);
                    }
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(lang.t("cmp")).strong());
                        let w = ui.available_width();
                        let ve = ui.add_sized([w, 24.0], egui::TextEdit::singleline(&mut self.verify_in).hint_text(lang.t("hint")));
                        ve.context_menu(|ui| {
                            if ui.button(lang.t("paste")).clicked() { ve.request_focus(); ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste); ui.close_menu(); }
                            if ui.button(lang.t("clear_in")).clicked() { self.verify_in.clear(); ui.close_menu(); }
                        });
                    });
                    let q = self.verify_in.trim().to_lowercase();
                    if !q.is_empty() {
                        ui.add_space(4.0);
                        let mut hit_algo: Option<String> = None;
                        let mut hit_file: Option<String> = None;
                        if !self.is_archive {
                            for (h, lb) in [(&self.crc32, "CRC32"), (&self.md5, "MD5"), (&self.sha1, "SHA1"), (&self.sha256, "SHA256")] {
                                if !h.is_empty() && h.to_lowercase() == q {
                                    hit_algo = Some(lb.to_string());
                                    hit_file = std::path::Path::new(&self.file_path).file_name().map(|n| n.to_string_lossy().to_string());
                                    break;
                                }
                            }
                        }
                        if hit_algo.is_none() {
                            if let Ok(l) = self.archive_list.lock() {
                                'o: for r in l.iter() {
                                    for (h, lb) in [(r.crc32.as_ref(), "CRC32"), (r.md5.as_ref(), "MD5"), (r.sha1.as_ref(), "SHA1"), (r.sha256.as_ref(), "SHA256")] {
                                        if let Some(x) = h {
                                            if x.to_lowercase() == q {
                                                hit_algo = Some(lb.to_string());
                                                hit_file = Some(r.path.clone());
                                                break 'o;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if let Some(algo) = hit_algo {
                            ui.colored_label(Color32::from_rgb(22, 163, 74), RichText::new(format!("{} ({})", lang.t("ok"), algo)).strong());
                            if let Some(fname) = hit_file {
                                ui.colored_label(Color32::from_rgb(22, 163, 74), RichText::new(fname).size(12.0));
                            }
                        } else {
                            ui.colored_label(Color32::from_rgb(220, 38, 38), RichText::new(lang.t("no")).strong());
                        }
                    }
                });
            }
            let arc_len = self.archive_list.lock().map(|v| v.len()).unwrap_or(0);
if arc_len > 0 {
ui.add_space(8.0);
ui.label(RichText::new(format!("{} ({})", lang.t("arc_list"), arc_len)).strong().color(Color32::from_rgb(30, 58, 138)));
egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
if let Ok(l) = self.archive_list.lock() {
for (i, r) in l.iter().enumerate() {
let esum = [
if self.show_crc32 { r.crc32.as_ref().map(|v| format!("CRC32:  {}", v)).unwrap_or_default() } else { String::new() },
if self.show_md5 { r.md5.as_ref().map(|v| format!("MD5:    {}", v)).unwrap_or_default() } else { String::new() },
if self.show_sha1 { r.sha1.as_ref().map(|v| format!("SHA1:   {}", v)).unwrap_or_default() } else { String::new() },
if self.show_sha256 { r.sha256.as_ref().map(|v| format!("SHA256: {}", v)).unwrap_or_default() } else { String::new() },
].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n");
Frame::none().fill(Color32::WHITE).stroke(Stroke::new(1.0_f32, Color32::from_rgb(203, 213, 225))).rounding(Rounding::same(8.0)).inner_margin(Margin::same(10.0)).show(ui, |ui| {
ui.horizontal(|ui| {
                                    let size_text = format_size(r.size);
                                    let size_w = ui.fonts(|f| f.layout_no_wrap(size_text.clone(), egui::FontId::proportional(11.5), Color32::GRAY).size().x) + 18.0;
                                    let max_name_w = (ui.available_width() - size_w).max(60.0);
                                    let disp_name = truncate_front_to_fit(ui, &r.path, egui::FontId::proportional(13.0), max_name_w);
                                    ui.label(RichText::new(disp_name).strong()).on_hover_text(&r.path);
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(RichText::new(size_text).size(11.5));
                                    });
                                });
ui.separator();
if self.show_crc32 { if let Some(v) = &r.crc32 { draw_hash_row(ui, &format!("a{}_c", i), "CRC32", v, &esum, &mut self.row_notice, lang); } }
if self.show_md5 { if let Some(v) = &r.md5 { draw_hash_row(ui, &format!("a{}_m", i), "MD5", v, &esum, &mut self.row_notice, lang); } }
if self.show_sha1 { if let Some(v) = &r.sha1 { draw_hash_row(ui, &format!("a{}_s1", i), "SHA1", v, &esum, &mut self.row_notice, lang); } }
if self.show_sha256 { if let Some(v) = &r.sha256 { draw_hash_row(ui, &format!("a{}_s2", i), "SHA256", v, &esum, &mut self.row_notice, lang); } }
});
ui.add_space(6.0);
}
}
});
}
// 가로 콘텐츠가 현재 창 너비를 넘을 경우 세로는 유지하고 가로만 자동 확장
let needed_w = ui.min_rect().width() + 24.0;
if let Some(inner_rect) = ctx.input(|i| i.viewport().inner_rect) {
let cur_w = inner_rect.width();
let cur_h = inner_rect.height();
if needed_w > cur_w + 4.0 && needed_w <= 1600.0 {
ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(needed_w.ceil(), cur_h)));
}
}
});
if self.show_feedback {
let mut open_fb = self.show_feedback;
let mut close_fb = false;
egui::Window::new(lang.t("fb_title")).open(&mut open_fb).collapsible(false).resizable(false).movable(true).default_pos([180.0, 110.0]).show(ctx, |ui| {
ui.add_space(4.0);
ui.add(egui::Label::new(RichText::new(lang.t("fb_desc")).size(12.5)).wrap());
ui.add_space(8.0);
Frame::none().fill(Color32::from_rgb(241, 245, 249)).stroke(Stroke::new(1.0_f32, Color32::from_rgb(203, 213, 225))).rounding(Rounding::same(6.0)).inner_margin(Margin::symmetric(12.0, 8.0)).show(ui, |ui| {
ui.horizontal(|ui| {
ui.label(RichText::new(APP_EMAIL).monospace().strong().size(13.5).color(Color32::from_rgb(30, 58, 138)));
if ui.button(lang.t("copy_email")).clicked() {
ui.output_mut(|o| o.copied_text = APP_EMAIL.to_string());
self.fb_copied = true;
}
if self.fb_copied {
ui.label(RichText::new(lang.t("copied_done")).color(Color32::from_rgb(22, 163, 74)).strong().size(12.0));
}
});
});
ui.add_space(10.0);
ui.vertical_centered(|ui| {
if ui.add(egui::Button::new(lang.t("close")).min_size(egui::vec2(90.0, 26.0))).clicked() { close_fb = true; }
});
});
self.show_feedback = open_fb && !close_fb;
}
if self.show_about {
            let mut open_about = self.show_about;
            let mut close_clicked = false;
            egui::Window::new(lang.t("about_t"))
                .open(&mut open_about)
                .collapsible(false)
                .resizable(false)
                .movable(true)
                .default_pos([180.0, 110.0])
                .show(ctx, |ui| {
                    ui.heading(RichText::new(format!("Hash Checker v{}", APP_VERSION)).color(Color32::from_rgb(30, 58, 138)));
                    ui.label(lang.t("sub")); ui.separator();
                    ui.label(format!("{}: {}", lang.t("author_l"), APP_AUTHOR));
                    ui.label(format!("{}: {}", lang.t("date_l"), lang.date()));
                    ui.label(format!("{}: {}", lang.t("email_l"), APP_EMAIL));
                    ui.label(RichText::new(lang.t("lic")).color(Color32::from_rgb(22, 163, 74)).strong());
                    ui.label(RichText::new("Copyright  2026 kwon0fe. All rights reserved.").color(Color32::from_rgb(100, 116, 139)).size(11.5));
                    ui.add_space(6.0); ui.separator();
                    ui.add(egui::Label::new(RichText::new(lang.t("don_msg")).size(11.5).color(Color32::from_rgb(71, 85, 105))).wrap());
                    ui.add_space(4.0);
                    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 28.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                        ui.horizontal(|ui| {
                            let group_w = if lang == Language::Ko { 236.0 } else { 145.0 };
                            let pad = ((ui.available_width() - group_w) * 0.5).max(0.0);
                            ui.add_space(pad);
                            if lang == Language::Ko {
                                let kp_btn = egui::Button::new(RichText::new(lang.t("don_kp")).color(Color32::from_rgb(25, 25, 25)).strong().size(12.0)).min_size(egui::vec2(114.0, 26.0)).fill(Color32::from_rgb(254, 229, 0));
                                if ui.add(kp_btn).clicked() {
                                    self.show_qr = !self.show_qr;
                                    if self.show_qr && self.qr_tex.is_none() {
                                        if let Ok(img) = image::load_from_memory(KAKAOPAY_QR_BYTES) {
                                            let rgba = img.to_rgba8();
                                            let size = [rgba.width() as usize, rgba.height() as usize];
                                            let cimg = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                                            self.qr_tex = Some(ctx.load_texture("kakaopay_qr", cimg, egui::TextureOptions::LINEAR));
                                        }
                                    }
                                }
                            }
                            let pp_btn = egui::Button::new(RichText::new(lang.t("don_pp")).color(Color32::WHITE).strong().size(12.0)).min_size(egui::vec2(114.0, 26.0)).fill(Color32::from_rgb(0, 112, 186));
                            if ui.add(pp_btn).clicked() {
                                self.show_qr = false;
                                #[cfg(target_os = "windows")]
                                { let mut c = Command::new("cmd"); c.args(["/C", "start", "", PAYPAL_URL]).creation_flags(0x08000000); let _ = c.spawn(); }
                                #[cfg(not(target_os = "windows"))]
                                ctx.open_url(egui::OpenUrl::new_tab(PAYPAL_URL));
                            }
                        });
                    });
                    if self.show_qr && lang == Language::Ko {
                        if let Some(tex) = &self.qr_tex {
                            ui.add_space(6.0);
                            ui.vertical_centered(|ui| {
                                ui.add(egui::Image::new(tex).fit_to_exact_size(egui::vec2(170.0, 170.0)));
                                ui.add_space(4.0);
                                ui.label(RichText::new("스마트폰 카메라 또는 카카오톡으로 QR 코드를 찍어 모바일로 접속해 주세요.").size(11.5).strong().color(Color32::from_rgb(30, 58, 138)));
                            });
                        }
                    }
                    ui.add_space(8.0);
                    ui.vertical_centered(|ui| {
                        if ui.add(egui::Button::new(lang.t("close")).min_size(egui::vec2(90.0, 26.0))).clicked() { close_clicked = true; }
                    });
                });
            self.show_about = open_about && !close_clicked;
        }
    }
}
impl HashCheckerApp {
fn reset_results_only(&mut self) { self.main_size = 0; self.is_archive = false; self.crc32.clear(); self.md5.clear(); self.sha1.clear(); self.sha256.clear(); if let Ok(mut l) = self.archive_list.lock() { l.clear(); } self.verify_in.clear(); self.progress = 0.0; self.status.clear(); self.notice_kind = NoticeKind::None; self.row_notice.clear(); self.start_time = None; self.elapsed = Duration::ZERO; }
fn full_reset(&mut self) { self.file_path.clear(); self.auto_run = false; self.reset_results_only(); }
fn start_job(&mut self) {
self.computing = true; self.progress = 0.0; self.start_time = Some(Instant::now()); self.elapsed = Duration::ZERO;
let cfg = AppConfig { crc32: self.show_crc32, md5: self.show_md5, sha1: self.show_sha1, sha256: self.show_sha256, lang: self.lang };
let p = PathBuf::from(&self.file_path);
let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
self.is_archive = ext == "zip" || ext == "7z";
let _ = self.tx.send(if self.is_archive { HashTask::Archive(p, cfg) } else { HashTask::File(p, cfg) });
}
fn save_txt(&mut self) {
if self.file_path.is_empty() { return; }
if let Some(p) = rfd::FileDialog::new().set_file_name("hash_results.txt").save_file() {
let mut out = format!("File: {}\n---\n", self.file_path);
if !self.is_archive {
out = format!("File: {}\nSize: {}\n---\n", self.file_path, format_size(self.main_size));
if self.show_crc32 && !self.crc32.is_empty() { out.push_str(&format!("CRC32:  {}\n", self.crc32)); }
if self.show_md5 && !self.md5.is_empty() { out.push_str(&format!("MD5:    {}\n", self.md5)); }
if self.show_sha1 && !self.sha1.is_empty() { out.push_str(&format!("SHA1:   {}\n", self.sha1)); }
if self.show_sha256 && !self.sha256.is_empty() { out.push_str(&format!("SHA256: {}\n", self.sha256)); }
}
if let Ok(l) = self.archive_list.lock() { if !l.is_empty() { out.push_str(&format!("\n=== {} ===\n", self.lang.t("arc_list"))); for r in l.iter() { out.push_str(&format!("\n[{}] ({})\n", r.path, format_size(r.size))); if let Some(v) = &r.crc32 { out.push_str(&format!("  CRC32:  {}\n", v)); } if let Some(v) = &r.md5 { out.push_str(&format!("  MD5:    {}\n", v)); } if let Some(v) = &r.sha1 { out.push_str(&format!("  SHA1:   {}\n", v)); } if let Some(v) = &r.sha256 { out.push_str(&format!("  SHA256: {}\n", v)); } } } }
if std::fs::write(p, out).is_ok() { self.notice_kind = NoticeKind::SavedOk; self.row_notice.clear(); }
}
}
}
fn setup_fonts(ctx: &egui::Context) {
#[cfg(target_os = "windows")]
{
let mut fonts = FontDefinitions::default();
for (nm, fp) in [("malgun", r"C:\Windows\Fonts\malgun.ttf"), ("meiryo", r"C:\Windows\Fonts\meiryo.ttc"), ("msyh", r"C:\Windows\Fonts\msyh.ttc")] {
if let Ok(d) = std::fs::read(fp) { fonts.font_data.insert(nm.into(), FontData::from_owned(d)); fonts.families.entry(FontFamily::Proportional).or_default().insert(0, nm.into()); fonts.families.entry(FontFamily::Monospace).or_default().insert(0, nm.into()); }
}
ctx.set_fonts(fonts);
}
}
fn main() -> Result<(), eframe::Error> {
let args = Args::parse();
if args.register_context_menu { let _ = register_context_menu(); return Ok(()); }
if args.unregister_context_menu { let _ = unregister_context_menu(); return Ok(()); }
let mut opt = eframe::NativeOptions::default();
opt.vsync = true;
let mut vp = egui::ViewportBuilder::default().with_inner_size([750.0, 620.0]).with_min_inner_size([620.0, 380.0]).with_resizable(true);
    if let Ok(img) = image::load_from_memory(EMBED_ICON_ICO) {
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        vp = vp.with_icon(egui::IconData { rgba: rgba.into_raw(), width: w, height: h });
    }
    opt.viewport = vp;
let init_path = args.path.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
eframe::run_native("Hash Checker", opt, Box::new(move |cc| {
setup_fonts(&cc.egui_ctx);
#[cfg(target_os = "windows")] sync_context_menu_if_registered();
let mut app = HashCheckerApp::default();
app.file_path = init_path;
Ok(Box::new(app))
}))
}














