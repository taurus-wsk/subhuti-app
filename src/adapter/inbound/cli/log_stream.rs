use colored::Colorize;
use serde_json::Value;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

pub fn run(
    trace_id: Option<String>,
    user_id: Option<String>,
    level: Option<String>,
    keyword: Option<String>,
    log_dir: String,
    tail: usize,
) -> anyhow::Result<()> {
    let log_path = Path::new(&log_dir);
    if !log_path.exists() {
        println!("{}", format!("❌ 日志目录不存在: {}", log_dir).red());
        return Ok(());
    }

    let mut log_files: Vec<_> = fs::read_dir(log_path)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension()?.to_str()? == "log" {
                Some(path)
            } else {
                None
            }
        })
        .collect();

    if log_files.is_empty() {
        println!("{}", "⚠️  未找到日志文件".yellow());
        return Ok(());
    }

    log_files.sort();
    let latest_file = log_files.last().unwrap();

    println!(
        "{}",
        format!("📜 监控日志: {}", latest_file.display())
            .yellow()
            .bold()
    );
    println!("───────────────────────────────────────────────────────────────");

    let (tx, rx) = mpsc::channel();

    let file_path = latest_file.clone();
    let tail_clone = tail;
    let trace_id_clone = trace_id.clone();
    let user_id_clone = user_id.clone();
    let level_clone = level.clone();
    let keyword_clone = keyword.clone();

    thread::spawn(move || {
        let mut file = File::open(&file_path).unwrap();
        let file_len = file.seek(SeekFrom::End(0)).unwrap();
        let start_pos = file_len.saturating_sub(tail_clone as u64 * 1024);
        file.seek(SeekFrom::Start(start_pos)).unwrap();

        let mut reader = BufReader::new(file);
        let mut line = String::new();

        loop {
            line.clear();
            let bytes_read = reader.read_line(&mut line).unwrap();

            if bytes_read == 0 {
                thread::sleep(Duration::from_millis(500));
                continue;
            }

            let trimmed_line = line.trim();
            if trimmed_line.is_empty() {
                continue;
            }

            let should_show = match serde_json::from_str::<Value>(trimmed_line) {
                Ok(json) => {
                    if let Some(trace_id) = &trace_id_clone {
                        if json["trace_id"].as_str() != Some(trace_id) {
                            continue;
                        }
                    }
                    if let Some(user_id) = &user_id_clone {
                        if json["user_id"].as_str() != Some(user_id) {
                            continue;
                        }
                    }
                    if let Some(level) = &level_clone {
                        if json["level"].as_str() != Some(level) {
                            continue;
                        }
                    }
                    if let Some(keyword) = &keyword_clone {
                        if !trimmed_line.contains(keyword) {
                            continue;
                        }
                    }
                    true
                }
                Err(_) => {
                    if let Some(keyword) = &keyword_clone {
                        if !trimmed_line.contains(keyword) {
                            continue;
                        }
                    }
                    true
                }
            };

            if should_show {
                tx.send(trimmed_line.to_string()).unwrap();
            }
        }
    });

    while let Ok(line) = rx.recv() {
        print_log_line(&line);
    }

    Ok(())
}

fn print_log_line(line: &str) {
    match serde_json::from_str::<Value>(line) {
        Ok(json) => {
            let timestamp = json["timestamp"].as_str().unwrap_or("");
            let level = json["level"].as_str().unwrap_or("");
            let message = json["message"].as_str().unwrap_or(line);
            let trace_id = json["trace_id"].as_str().unwrap_or("");
            let user_id = json["user_id"].as_str().unwrap_or("");

            let level_color = match level.to_lowercase().as_str() {
                "error" => "red",
                "warn" => "yellow",
                "info" => "green",
                "debug" => "blue",
                "trace" => "cyan",
                _ => "white",
            };

            if !trace_id.is_empty() {
                println!(
                    "[{}] [{}] [{}] [{}] {}",
                    timestamp,
                    level.color(level_color),
                    trace_id.purple(),
                    user_id.cyan(),
                    message
                );
            } else {
                println!("[{}] [{}] {}", timestamp, level.color(level_color), message);
            }
        }
        Err(_) => {
            println!("{}", line);
        }
    }
}
