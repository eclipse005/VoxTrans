//! Demo v2: 术语提取的"分窗增量"架构。
//!
//! 设计原则（源头拒绝问题，不打补丁）：
//! 1. 工作单元 = 小窗调用（prompt ~1-1.5K token，任何端点的安全形状）
//! 2. 窗宽阶梯：失败即缩窗重试（4000 → 2000 → 1200 字符下限），
//!    系统永远不会两次向端点索要它吃不下的请求
//! 3. 增量上下文：每窗只携带"已确定术语表"的紧凑摘要，不重放历史
//! 4. 饱和早停：连续 3 窗零新术语即停止，长视频成本不随时长线性爆炸
//! 5. 语言无关：术语识别交给模型，本地只做确定性工作（去重/计数核对）
//!
//! 对照组：旧术语 Agent（EP7 实测 57.6K~110.2K token，8 轮循环 + web_search）。
//!
//! 用法：cargo run --example terminology_fast_demo [-- <db> <task> <base_url> <key> <model>]

use std::collections::BTreeMap;
use std::time::Instant;

use serde_json::{Value, json};
use sqlx::sqlite::SqlitePoolOptions;

const WINDOW_START_CHARS: usize = 4000;
const WINDOW_MIN_CHARS: usize = 1200;
const CALL_TIMEOUT_SECS: u64 = 150;
const EARLY_EXIT_EMPTY_WINDOWS: usize = 3;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let db_path = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "C:\\Users\\ADMIN\\AppData\\Roaming\\com.voxtrans.desktop\\voxtrans.db".into());
    let task_id = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| "1789642800138-wmn6tg".into());

    // ── 1. 真实数据 ──────────────────────────────────────────────────────────
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&format!("sqlite://{db_path}"))
        .await?;
    let setting: (String, String, String) = sqlx::query_as(
        "SELECT translate_base_url, translate_api_key, translate_model FROM settings LIMIT 1",
    )
    .fetch_one(&pool)
    .await?;
    let (db_base_url, db_api_key, db_model) = setting;
    let base_url = args.get(3).cloned().unwrap_or(db_base_url);
    let api_key = args.get(4).cloned().unwrap_or(db_api_key);
    let model = args.get(5).cloned().unwrap_or(db_model);
    eprintln!("== 配置 ==");
    eprintln!("endpoint: {base_url}");
    eprintln!("model:    {model}");
    eprintln!("key:      {}…", &api_key[..api_key.len().min(8)]);

    let rows: Vec<(i64, String)> =
        sqlx::query_as("SELECT segment_index, text FROM asr_transcripts WHERE task_id = ? ORDER BY segment_index")
            .bind(&task_id)
            .fetch_all(&pool)
            .await?;
    if rows.is_empty() {
        return Err(format!("task {task_id} 没有转写数据").into());
    }
    let transcript: String = rows.into_iter().map(|(_, t)| t).collect::<Vec<_>>().join("\n");
    eprintln!("transcript: {} 字符（task {task_id}）", transcript.len());

    // ── 2. 切窗（句子边界对齐，源头就是小工作单元）──────────────────────────
    let cues = split_sentences(&transcript);
    eprintln!("句子 cue: {} 条", cues.len());

    // ── 3. 分窗增量抽取 ─────────────────────────────────────────────────────
    let t0 = Instant::now();
    let mut glossary: BTreeMap<String, (String, String)> = BTreeMap::new(); // lower_source -> (source, target+note)
    let mut misheard: Vec<(String, String, String)> = Vec::new();
    let mut style_guide = String::new();
    let mut window_chars = WINDOW_START_CHARS;
    let mut total_prompt = 0u64;
    let mut total_completion = 0u64;
    let mut calls = 0u32;
    let mut empty_streak = 0usize;
    let mut i = 0usize;
    let mut window_no = 0usize;

    while i < cues.len() {
        // 装一个窗（按当前窗宽）
        let mut chars = 0usize;
        let mut j = i;
        while j < cues.len() {
            let add = cues[j].chars().count() + 1;
            if !cues[i..j].is_empty() && chars + add > window_chars {
                break;
            }
            chars += add;
            j += 1;
        }
        let window_text = cues[i..j].join("\n");
        window_no += 1;

        // 已确定术语表的紧凑摘要（增量上下文，不是历史重放）
        let known: Vec<String> = glossary
            .values()
            .map(|(s, t)| format!("{s} -> {t}"))
            .collect();
        let prompt = build_window_prompt(&window_text, &known, window_no == 1);

        // 调用：失败缩窗重试同窗（阶梯），成功才前进
        match call_llm(&base_url, &api_key, &model, &prompt).await {
            Ok(resp) => {
                calls += 1;
                let usage = &resp["usage"];
                total_prompt += usage["prompt_tokens"].as_u64().unwrap_or(0);
                total_completion += usage["completion_tokens"].as_u64().unwrap_or(0);
                let content = resp["choices"][0]["message"]["content"].as_str().unwrap_or_default();
                let parsed = parse_json_loose(content)
                    .ok_or_else(|| format!("窗 {window_no} 输出不是合法 JSON: {}", truncate(content, 200)))?;

                if window_no == 1 {
                    style_guide = parsed["styleGuide"].as_str().unwrap_or_default().trim().to_string();
                }
                let mut new_terms = 0usize;
                for entry in parsed["terms"].as_array().cloned().unwrap_or_default() {
                    let source = entry["source"].as_str().unwrap_or_default().trim().to_string();
                    if source.is_empty() {
                        continue;
                    }
                    let target = entry["target"].as_str().unwrap_or_default().trim().to_string();
                    let note = entry["note"].as_str().unwrap_or_default().trim().to_string();
                    let key = source.to_lowercase();
                    if let std::collections::btree_map::Entry::Vacant(slot) = glossary.entry(key) {
                        let note_suffix = if note.is_empty() { String::new() } else { format!("  ｜{note}") };
                        slot.insert((source.clone(), format!("{target}{note_suffix}")));
                        new_terms += 1;
                    }
                }
                for pair in parsed["misheard"].as_array().cloned().unwrap_or_default() {
                    let wrong = pair["wrong"].as_str().unwrap_or_default().trim().to_string();
                    let right = pair["right"].as_str().unwrap_or_default().trim().to_string();
                    let note = pair["note"].as_str().unwrap_or_default().trim().to_string();
                    if !wrong.is_empty() && !misheard.iter().any(|(w, _, _)| w.eq_ignore_ascii_case(&wrong)) {
                        misheard.push((wrong, right, note));
                    }
                }
                eprintln!(
                    "窗 {window_no} [cue {}..{}] {} 字符: prompt={} completion={} 新术语={new_terms}",
                    i + 1,
                    j,
                    window_text.chars().count(),
                    usage["prompt_tokens"],
                    usage["completion_tokens"],
                );
                empty_streak = if new_terms == 0 { empty_streak + 1 } else { 0 };
                if empty_streak >= EARLY_EXIT_EMPTY_WINDOWS {
                    eprintln!(
                        "饱和早停：连续 {empty_streak} 窗零新术语，剩余 {} 条 cue 跳过（术语表已收敛）",
                        cues.len() - j
                    );
                    i = cues.len();
                    continue;
                }
                i = j;
            }
            Err(err) => {
                // 阶梯：缩窗重试同一位置；到底后仍失败才报错
                let halved = window_chars / 2;
                if halved < WINDOW_MIN_CHARS {
                    return Err(format!("窗宽已到下限 {WINDOW_MIN_CHARS} 仍失败: {err}").into());
                }
                eprintln!(
                    "窗 {window_no} 失败（{} 字符）: {}；窗宽 {window_chars} → {halved}，重试同位置",
                    window_text.chars().count(),
                    truncate(&err.to_string(), 160)
                );
                window_chars = halved;
            }
        }
    }

    // ── 4. 本地校验 + 报告 ───────────────────────────────────────────────────
    eprintln!(
        "\n== 汇总 ==\n调用 {calls} 次（窗宽最终 {window_chars} 字符），耗时 {} ms",
        t0.elapsed().as_millis()
    );
    eprintln!("token: prompt={total_prompt} completion={total_completion} total={}", total_prompt + total_completion);

    let lower_all = transcript.to_lowercase();
    eprintln!("\n== glossary（{} 条，[x] 为全片本地核对次数）==", glossary.len());
    for (source, target_note) in glossary.values() {
        let (target, note) = match target_note.split_once("  ｜") {
            Some((t, n)) => (t.to_string(), n.to_string()),
            None => (target_note.clone(), String::new()),
        };
        let hits = count_occurrences(&lower_all, &source.to_lowercase());
        let flag = if hits == 0 { "  <-- 0 命中，剔除" } else { "" };
        eprintln!("  {source} -> {target}  [x{hits}]{flag}{}", if note.is_empty() { String::new() } else { format!("  {note}") });
    }
    if !misheard.is_empty() {
        eprintln!("\n== 误听判定（{} 组）==", misheard.len());
        for (wrong, right, note) in &misheard {
            eprintln!("  {wrong} -> {right}  {note}");
        }
    }
    if !style_guide.is_empty() {
        eprintln!("\n== styleGuide ==\n  {style_guide}");
    }
    eprintln!(
        "\n== 成本对照 ==\n分窗增量: prompt={total_prompt} completion={total_completion} total={}\n旧 Agent(EP7): -fast 57.6K；v4.1 110.2K",
        total_prompt + total_completion
    );
    Ok(())
}

// ── 窗 prompt ───────────────────────────────────────────────────────────────

fn build_window_prompt(window_text: &str, known_terms: &[String], first_window: bool) -> String {
    let known_block = if known_terms.is_empty() {
        "  （尚无）".to_string()
    } else {
        known_terms.join("\n")
    };
    let style_task = if first_window {
        "\n3. 另外给出 styleGuide：这个视频译成目标语言的口吻、句式、术语处理原则（100 字内）。"
    } else {
        ""
    };
    let style_field = if first_window {
        ",\"styleGuide\":\"...\""
    } else {
        ""
    };
    format!(
        "你是字幕术语审校。下面是视频转写的一个连续片段。已确定的术语表（此前片段提取）附后。\n\
任务：\n\
1. 只提取本片段中【新出现】的领域术语/专有名词/缩写——已确定术语表里有的不要重复输出；普通常见词不要收录；只收录影响理解或会反复出现的词。给出 zh-CN 译名（采用中文交易/技术社区惯用译法，不要字面直译；行业缩写保留原文），note 写译法依据（没有就空字符串）。\n\
2. 本片段中如有疑似语音识别误听的词（上下文里讲不通的词），给出裁决；没有就输出空数组。{style_task}\n\
只返回 JSON：{{\"terms\":[{{\"source\":\"...\",\"target\":\"...\",\"note\":\"...\"}}],\"misheard\":[{{\"wrong\":\"...\",\"right\":\"...\",\"note\":\"...\"}}]{style_field}}}\n\
\n== 已确定术语表 ==\n{known_block}\n\
\n== 本片段转写 ==\n{window_text}"
    )
}

// ── LLM 调用（小窗 + 429/超时重试；缩窗由外层阶梯负责）─────────────────────

async fn call_llm(base_url: &str, api_key: &str, model: &str, prompt: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let trimmed = base_url.trim_end_matches('/');
    let url = if trimmed.to_ascii_lowercase().ends_with("/chat/completions") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/chat/completions")
    };
    let body = json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "temperature": 0.2,
        "stream": false,
    });
    for attempt in 1..=3u32 {
        let sent = reqwest::Client::new()
            .post(&url)
            .bearer_auth(api_key)
            .json(&body)
            .timeout(std::time::Duration::from_secs(CALL_TIMEOUT_SECS))
            .send()
            .await;
        let resp = match sent {
            Ok(resp) => resp,
            Err(err) => {
                if attempt == 3 {
                    return Err(format!("请求失败（{attempt} 次尝试）: {err}").into());
                }
                eprintln!("  请求失败（第 {attempt} 次），20s 后重试");
                tokio::time::sleep(std::time::Duration::from_secs(20)).await;
                continue;
            }
        };
        let status = resp.status();
        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .map(|s| s.min(30)) // 退避上限 30s：端点可能给出按小时计的 retry-after，照睡会挂死
            .unwrap_or(20);
        let value: Value = resp.json().await.unwrap_or(Value::Null);
        if status.is_success() {
            return Ok(value);
        }
        let retryable = status.as_u16() == 429 || status.is_server_error();
        let detail = serde_json::to_string(&value).unwrap_or_default();
        if !retryable || attempt == 3 {
            return Err(format!("http {status}: {}", truncate(&detail, 200)).into());
        }
        eprintln!("  http {status}（第 {attempt} 次），{retry_after}s 后重试: {}", truncate(&detail, 120));
        tokio::time::sleep(std::time::Duration::from_secs(retry_after)).await;
    }
    unreachable!("retry loop always returns")
}

// ── 工具函数 ────────────────────────────────────────────────────────────────

/// 按句子边界切 cue（语言无关：.?!?＋中文句号问号叹号）
fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut buf = String::new();
    let mut prev_upper = false;
    for ch in text.chars() {
        buf.push(ch);
        let terminator = matches!(ch, '.' | '!' | '?' | '。' | '！' | '？');
        if terminator {
            // 缩写保护：句点前是单个大写字母（如 "U.S."）则不切
            let is_abbrev = ch == '.' && prev_upper;
            if !is_abbrev && buf.trim().chars().count() > 1 {
                sentences.push(buf.trim().to_string());
                buf.clear();
            }
        }
        prev_upper = ch.is_ascii_uppercase();
    }
    if !buf.trim().is_empty() {
        sentences.push(buf.trim().to_string());
    }
    sentences.into_iter().filter(|s| !s.is_empty()).collect()
}

/// 宽松提取模型输出里的 JSON 对象（容忍 ``` 包裹或前后杂文本）。
fn parse_json_loose(text: &str) -> Option<Value> {
    let trimmed = text.trim();
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        return Some(v);
    }
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    serde_json::from_str::<Value>(&trimmed[start..=end]).ok()
}

fn count_occurrences(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let mut count = 0usize;
    let mut pos = 0;
    while let Some(found) = haystack[pos..].find(needle) {
        count += 1;
        pos += found + needle.len();
    }
    count
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}
