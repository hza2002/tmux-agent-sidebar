//! Stateful JSONL adapters. Context and the cursor live in the same file cache.
use super::*;

#[derive(Debug, Clone, Default)]
pub(super) struct Context {
    model: String,
    turn: String,
    session: String,
    account: Option<String>,
    tier: Option<String>,
    total: Option<String>,
}

pub(super) fn parse_line(
    source: Source,
    line: &str,
    path: &Path,
    context: &mut Context,
    out: &mut Vec<Record>,
    incomplete: &mut bool,
) {
    if ![
        "\"session_meta\"",
        "\"turn_context\"",
        "\"token_count\"",
        "\"thread_settings_applied\"",
        "\"usage\"",
    ]
    .iter()
    .any(|s| line.contains(s))
    {
        return;
    }
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        *incomplete = true;
        return;
    };
    match source {
        Source::Codex => codex(&v, path, context, out, incomplete),
        Source::Claude => claude(&v, out, incomplete),
    }
}

fn codex(v: &Value, path: &Path, c: &mut Context, out: &mut Vec<Record>, incomplete: &mut bool) {
    let p = &v["payload"];
    match v["type"].as_str() {
        Some("session_meta") => {
            c.session = p["id"].as_str().unwrap_or_default().into();
            c.account = p["creator_account_id"].as_str().map(str::to_owned);
        }
        Some("turn_context") => {
            if let Some(model) = p["model"].as_str() {
                c.model = model.to_ascii_lowercase();
            }
            if let Some(turn) = p["turn_id"].as_str() {
                c.turn = turn.into();
            }
            // Older/current turn_context events omit tier. Preserve the last
            // explicit settings event; only a present field overrides it.
            if let Some(tier) = p.get("service_tier") {
                c.tier = tier.as_str().map(str::to_owned);
            }
        }
        Some("event_msg") if p["type"] == "thread_settings_applied" => {
            let settings = &p["thread_settings"];
            c.tier = settings["service_tier"].as_str().map(str::to_owned);
            if let Some(model) = settings["model"].as_str() {
                c.model = model.to_ascii_lowercase();
            }
        }
        Some("event_msg") if p["type"] == "token_count" => {
            let Some(u) = p
                .pointer("/info/last_token_usage")
                .filter(|v| v.is_object())
            else {
                return;
            };
            let Some(raw_ts) = v["timestamp"].as_str() else {
                *incomplete = true;
                return;
            };
            let Some(ts) = parse_timestamp(raw_ts) else {
                *incomplete = true;
                return;
            };
            let Some(input) = u["input_tokens"].as_u64() else {
                *incomplete = true;
                return;
            };
            let Some(output) = u["output_tokens"].as_u64() else {
                *incomplete = true;
                return;
            };
            let cached = u["cached_input_tokens"].as_u64().unwrap_or(0);
            if cached > input {
                *incomplete = true;
                return;
            }
            let total = p
                .pointer("/info/total_token_usage")
                .filter(|v| v.is_object())
                .map(Value::to_string);
            // Quota-only updates repeat the last usage with unchanged cumulative
            // counters. They are not new model requests, even with a new timestamp.
            if total.is_some() && c.total == total {
                return;
            }
            c.total = total.clone();
            if c.model.is_empty() {
                *incomplete = true;
                return;
            }
            let ds = is_deepseek(&c.model);
            if !ds
                && !["gpt-", "codex-", "o1", "o3", "o4"]
                    .iter()
                    .any(|prefix| c.model.starts_with(prefix))
            {
                return;
            }
            let tokens = Tokens {
                uncached: input - cached,
                cached,
                output,
            };
            if tokens.total() == 0 {
                return;
            }
            let scope = if c.turn.is_empty() {
                if c.session.is_empty() {
                    path.to_string_lossy().into_owned()
                } else {
                    c.session.clone()
                }
            } else {
                c.turn.clone()
            };
            let identity = format!(
                "{}|{}|{}",
                scope,
                c.model,
                total.as_deref().unwrap_or(raw_ts)
            );
            let meta = (!ds).then(|| metering::EventMeta {
                model: c.model.clone(),
                tier: c.tier.clone(),
                account: c.account.clone(),
                cache_write: u["cache_write_input_tokens"].as_u64().unwrap_or(0),
                uncertain_identity: c.turn.is_empty() || total.is_none(),
            });
            out.push(Record {
                codex: meta,
                ts,
                family: family_for(&c.model),
                tokens,
                id: RecordId::Codex {
                    identity: identity.into(),
                    ts: if total.is_some() { 0 } else { ts },
                    uncached: tokens.uncached,
                    cached,
                    output,
                },
            });
        }
        _ => {}
    }
}

fn claude(v: &Value, out: &mut Vec<Record>, incomplete: &mut bool) {
    let Some(model) = v.pointer("/message/model").and_then(Value::as_str) else {
        return;
    };
    if !is_deepseek(model) {
        return;
    }
    let Some(ts) = v["timestamp"].as_str().and_then(parse_timestamp) else {
        *incomplete = true;
        return;
    };
    let Some(u) = v.pointer("/message/usage").filter(|v| v.is_object()) else {
        return;
    };
    let (Some(input), Some(output)) = (u["input_tokens"].as_u64(), u["output_tokens"].as_u64())
    else {
        *incomplete = true;
        return;
    };
    let tokens = Tokens {
        uncached: input.saturating_add(u["cache_creation_input_tokens"].as_u64().unwrap_or(0)),
        cached: u["cache_read_input_tokens"].as_u64().unwrap_or(0),
        output,
    };
    if tokens.total() == 0 {
        return;
    }
    let id = v
        .pointer("/message/id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{}|{}|{:?}", v["timestamp"], model, tokens));
    out.push(Record {
        codex: None,
        ts,
        family: family_for(model),
        tokens,
        id: RecordId::Claude {
            message_id: id.into(),
        },
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn context() -> String {
        format!(
            "{}\n{}\n",
            json!({"type":"session_meta","payload":{"id":"session","creator_account_id":"account"}}),
            json!({"type":"turn_context","payload":{"model":"gpt-6.1-sol","turn_id":"turn","service_tier":"standard"}})
        )
    }
    fn event(ts: &str, total: u64) -> String {
        format!(
            "{}\n",
            json!({"timestamp":ts,"type":"event_msg","payload":{"type":"token_count","info":{
            "last_token_usage":{"input_tokens":100,"cached_input_tokens":60,"output_tokens":10},
            "total_token_usage":{"input_tokens":total,"cached_input_tokens":60,"output_tokens":10}}}})
        )
    }
    #[test]
    fn append_recovers_context_ignores_partial_tail_and_repeated_counters() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("old.jsonl");
        std::fs::write(&file, context()).unwrap();
        let mut scanner = Scanner::default();
        scanner.refresh_file(Source::Codex, &file, 0);
        let record = event("2026-10-26T01:00:00.123Z", 100);
        let split = record.len() / 2;
        let mut append = std::fs::OpenOptions::new()
            .append(true)
            .open(&file)
            .unwrap();
        append.write_all(&record.as_bytes()[..split]).unwrap();
        scanner.refresh_file(Source::Codex, &file, 0);
        assert!(scanner.files[&file].records.is_empty());
        append.write_all(&record.as_bytes()[split..]).unwrap();
        append
            .write_all(event("2026-10-26T01:00:01.456Z", 100).as_bytes())
            .unwrap();
        append
            .write_all(event("2026-10-26T01:00:01.789Z", 200).as_bytes())
            .unwrap();
        scanner.refresh_file(Source::Codex, &file, 0);
        assert_eq!(scanner.files[&file].records.len(), 2);
        let end = parse_timestamp("2026-10-26T02:00:00Z").unwrap();
        let summary = metering::summarize(&scanner, end, 0);
        assert_eq!(summary.recent[0].total.tokens.total(), 220);
        assert_eq!(summary.recent[0].total.unpriced, 0);
        // A restart and full replay produces the same metadata and aggregates.
        let mut rebuilt = Scanner::default();
        rebuilt.refresh_file(Source::Codex, &file, 0);
        assert_eq!(summary, metering::summarize(&rebuilt, end, 0));
    }
    #[test]
    fn copies_collapse_but_other_turns_count_and_rewrites_replace() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.jsonl");
        let b = dir.path().join("b.jsonl");
        let body = context() + &event("2026-10-26T01:00:00.123Z", 100);
        std::fs::write(&a, &body).unwrap();
        std::fs::write(&b, &body).unwrap();
        let mut scanner = Scanner::default();
        scanner.refresh_file(Source::Codex, &a, 0);
        scanner.refresh_file(Source::Codex, &b, 0);
        let end = parse_timestamp("2026-10-26T02:00:00Z").unwrap();
        assert_eq!(
            metering::summarize(&scanner, end, 0).recent[0]
                .total
                .tokens
                .total(),
            110
        );
        std::fs::write(&b, body.replace("\"turn\"", "\"other\"")).unwrap();
        scanner.refresh_file(Source::Codex, &b, 0);
        assert_eq!(
            metering::summarize(&scanner, end, 0).recent[0]
                .total
                .tokens
                .total(),
            220
        );
        std::fs::write(&a, context()).unwrap();
        scanner.refresh_file(Source::Codex, &a, 0);
        assert_eq!(
            metering::summarize(&scanner, end, 0).recent[0]
                .total
                .tokens
                .total(),
            110
        );
    }
    #[test]
    fn window_is_half_open_and_missing_tier_uses_standard() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("events.jsonl");
        let body = context().replace(",\"service_tier\":\"standard\"", "")
            + &event("2026-10-25T02:00:00.000Z", 100)
            + &event("2026-10-26T02:00:00.000Z", 200);
        std::fs::write(&file, body).unwrap();
        let mut scanner = Scanner::default();
        scanner.refresh_file(Source::Codex, &file, 0);
        let end = parse_timestamp("2026-10-26T02:00:00Z").unwrap();
        let result = metering::summarize(&scanner, end, 0);
        assert_eq!(result.recent[0].total.tokens.total(), 110);
        assert_eq!(result.recent[0].total.assumed_standard, 1);
        assert_eq!(result.recent[0].total.unpriced, 0);
    }
    #[test]
    fn discovery_finds_a_month_old_session_with_new_usage() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("2025/01/01");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(
            old.join("old.jsonl"),
            context() + &event("2026-10-26T01:00:00.123Z", 100),
        )
        .unwrap();
        let mut scanner = Scanner::default();
        let end = parse_timestamp("2026-10-26T02:00:00Z").unwrap();
        scanner
            .scan_roots(
                &[(Source::Codex, dir.path().into())],
                Window::Today,
                end,
                &|| false,
            )
            .unwrap();
        assert_eq!(
            metering::summarize(&scanner, end, 0).recent[0]
                .total
                .tokens
                .total(),
            110
        );
    }
    #[test]
    fn historical_settings_survive_context_and_fast_switches_split_costs() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("tier.jsonl");
        let settings = |tier: Option<&str>| {
            format!(
                "{}\n",
                json!({"type":"event_msg","payload":{"type":"thread_settings_applied","thread_settings":{"model":"gpt-6.1-sol","service_tier":tier}}})
            )
        };
        let turn = format!(
            "{}\n",
            json!({"type":"turn_context","payload":{"model":"gpt-6.1-sol","turn_id":"turn"}})
        );
        std::fs::write(
            &file,
            settings(Some("default")) + &turn + &event("2026-10-26T01:00:00Z", 100),
        )
        .unwrap();
        let mut scanner = Scanner::default();
        scanner.refresh_file(Source::Codex, &file, 0);
        use std::io::Write;
        let mut append = std::fs::OpenOptions::new()
            .append(true)
            .open(&file)
            .unwrap();
        append
            .write_all(
                (settings(Some("fast")) + &turn + &event("2026-10-26T01:01:00Z", 200)).as_bytes(),
            )
            .unwrap();
        scanner.refresh_file(Source::Codex, &file, 0);
        let now = parse_timestamp("2026-10-26T02:00:00Z").unwrap();
        let snapshot = metering::summarize(&scanner, now, 0);
        let total = &snapshot.recent[0].total;
        // One Standard and one Fast request with the same token composition.
        let base = pricing::cost(
            pricing::version(pricing::BOOK, "gpt-6.1-sol", now)
                .unwrap()
                .rates(now, 100),
            Tokens {
                uncached: 40,
                cached: 60,
                output: 10,
            },
        );
        assert_eq!(total.cost, base * 3);
        assert_eq!(total.unknown_tier, 0);
        append
            .write_all((settings(None) + &event("2026-10-26T01:02:00Z", 300)).as_bytes())
            .unwrap();
        scanner.refresh_file(Source::Codex, &file, 0);
        assert_eq!(
            metering::summarize(&scanner, now, 0).recent[0]
                .total
                .assumed_standard,
            1
        );
        let mut rebuilt = Scanner::default();
        rebuilt.refresh_file(Source::Codex, &file, 0);
        assert_eq!(
            metering::summarize(&scanner, now, 0),
            metering::summarize(&rebuilt, now, 0)
        );
    }
}
