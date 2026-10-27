//! Allowlisted, replayable metering evidence. No transcript text crosses this boundary.
use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::usage::{
    Record, RecordId, Source, Tokens, family_for, is_deepseek, metering, parse_timestamp,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Context {
    fields: BTreeMap<String, Value>,
    total: Option<CounterVector>,
    epoch: u64,
    ordinal: u64,
    last_key: Option<String>,
    last_usage: Option<CounterVector>,
    last_turn: Option<String>,
    fork_warning_emitted: bool,
    response: Option<ResponseContext>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct ResponseContext {
    id: String,
    session: Option<String>,
    usage: CounterVector,
    thread_usage: Option<CounterVector>,
    turn: Option<String>,
}

/// Optional counters remain optional; missing is not a reported zero.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct CounterVector {
    pub input: Option<u64>,
    pub cached: Option<u64>,
    pub cache_write: Option<u64>,
    pub cache_write_5m: Option<u64>,
    pub cache_write_1h: Option<u64>,
    pub output: Option<u64>,
    pub reasoning: Option<u64>,
    pub total: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Evidence {
    pub key: String,
    pub canonical: String,
    pub record: Option<Record>,
    pub candidate: Option<Record>,
    pub source: String,
    pub raw_ts: String,
    pub session: Option<String>,
    pub message: Option<String>,
    pub request: Option<String>,
    pub turn: Option<String>,
    pub response: Option<String>,
    pub related_response: Option<String>,
    pub related_key: Option<String>,
    pub source_path: String,
    pub offset: u64,
    pub kind: String,
    pub issue: Option<String>,
    pub preference: i64,
    pub sidechain: Option<bool>,
    pub parent_session: Option<String>,
    pub fork_ts: Option<String>,
    pub weak: bool,
    pub cumulative: Option<CounterVector>,
}

pub(super) struct Selection {
    pub key: String,
    pub record: Option<Record>,
    pub issue: Option<String>,
    pub selected: usize,
}

/// Reconcile a group already linked by identity/explicit response association.
/// Callers must not group records merely because their token counts match.
pub(super) fn reconcile(group: &[Evidence]) -> Option<Selection> {
    let first = group.first()?;
    let response = group
        .iter()
        .find_map(|e| e.response.as_deref().or(e.related_response.as_deref()));
    let owner = group
        .iter()
        .find(|e| e.kind == "token_usage_record")
        .or_else(|| group.iter().find(|e| e.related_response.is_some()))
        .unwrap_or(first);
    let key = response.map_or_else(
        || first.key.clone(),
        |id| response_key(owner.session.as_deref(), id, &owner.source_path),
    );
    let has_marker = group.iter().any(|e| e.kind == "compacted");
    let mut candidates = group.iter().enumerate().filter_map(|(index, e)| {
        let r = e.record.as_ref().or_else(|| {
            (has_marker && e.kind == "token_usage_record")
                .then_some(e.candidate.as_ref())
                .flatten()
        })?;
        // An ordinary token_count supersedes a countable compaction representation.
        let rank = i64::from(e.kind == "token_count");
        Some((index, e, r, rank))
    });
    let Some(mut chosen) = candidates.next() else {
        return Some(Selection {
            key,
            record: None,
            issue: None,
            selected: 0,
        });
    };
    let mut issue = None;
    for candidate in candidates {
        if same_record_usage(chosen.2, candidate.2) {
            if candidate.3 > chosen.3 {
                chosen = candidate;
            }
        } else if first.source == "claude" && candidate.1.preference > chosen.1.preference {
            chosen = candidate;
        } else if first.source != "claude" || candidate.1.preference == chosen.1.preference {
            issue = Some("conflicting metering evidence for one logical event".into());
        }
    }
    let mut record = chosen.2.clone();
    set_record_key(&mut record, &key);
    Some(Selection {
        key,
        record: Some(record),
        issue,
        selected: chosen.0,
    })
}

fn same_record_usage(a: &Record, b: &Record) -> bool {
    a.tokens == b.tokens && a.family == b.family && a.codex == b.codex
}

fn set_record_key(record: &mut Record, key: &str) {
    match &mut record.id {
        RecordId::Claude { message_id } => *message_id = key.into(),
        RecordId::Codex { identity, .. } => *identity = key.into(),
    }
}

pub(super) fn parse_line(
    source: Source,
    line: &str,
    path: &Path,
    offset: u64,
    context: &mut Context,
) -> Result<Vec<Evidence>, String> {
    // Parse every complete JSON line: a textual prefilter would miss newly shaped
    // metering records and could mistake embedded conversation text for metadata.
    let v: Value =
        serde_json::from_str(line).map_err(|_| "invalid JSON source record".to_owned())?;
    if !v.is_object() {
        return Ok(Vec::new());
    }
    match source {
        Source::Claude => parse_claude(&v, path, offset),
        Source::Codex => parse_codex(&v, path, offset, context),
    }
}

fn text(v: &Value) -> Option<String> {
    v.as_str().filter(|s| !s.is_empty()).map(str::to_owned)
}

fn key(parts: &[Value]) -> String {
    serde_json::to_string(parts).expect("JSON value serialization cannot fail")
}

fn response_key(session: Option<&str>, response: &str, path: &str) -> String {
    key(&[
        json!("codex_response"),
        json!(session.unwrap_or(path)),
        json!(response),
    ])
}

fn selected(v: &Value, fields: &[&str]) -> Value {
    let mut map = serde_json::Map::new();
    for name in fields {
        if let Some(value) = v
            .get(*name)
            .filter(|value| !value.is_array() && !value.is_object())
        {
            map.insert((*name).to_owned(), value.clone());
        }
    }
    Value::Object(map)
}

fn optional_counter(v: &Value, name: &str) -> Result<Option<u64>, String> {
    match v.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .filter(|n| *n <= i64::MAX as u64)
            .map(Some)
            .ok_or_else(|| format!("invalid or overflowing {name} counter")),
    }
}

fn aliased_counter(v: &Value, names: &[&str]) -> Result<Option<u64>, String> {
    let mut chosen = None;
    for name in names {
        if let Some(value) = optional_counter(v, name)? {
            if chosen.is_some_and(|old| old != value) {
                return Err("conflicting source counter aliases".into());
            }
            chosen = Some(value);
        }
    }
    Ok(chosen)
}

fn counters(v: &Value, claude: bool) -> Result<(CounterVector, Value), String> {
    if !v.is_object() {
        return Err("metering usage is not an object".into());
    }
    let cache_name = if claude {
        "cache_read_input_tokens"
    } else {
        "cached_input_tokens"
    };
    let write_name = if claude {
        "cache_creation_input_tokens"
    } else {
        "cache_write_input_tokens"
    };
    let ttl = &v["cache_creation"];
    let result = CounterVector {
        input: if claude {
            optional_counter(v, "input_tokens")?
        } else {
            aliased_counter(v, &["input_tokens", "prompt_tokens", "input"])?
        },
        cached: if claude {
            optional_counter(v, cache_name)?
        } else {
            aliased_counter(
                v,
                &[
                    "cached_input_tokens",
                    "cache_read_input_tokens",
                    "cached_tokens",
                ],
            )?
        },
        cache_write: if claude {
            optional_counter(v, write_name)?
        } else {
            aliased_counter(
                v,
                &["cache_write_input_tokens", "cache_creation_input_tokens"],
            )?
        },
        cache_write_5m: optional_counter(ttl, "ephemeral_5m_input_tokens")?,
        cache_write_1h: optional_counter(ttl, "ephemeral_1h_input_tokens")?,
        output: if claude {
            optional_counter(v, "output_tokens")?
        } else {
            aliased_counter(v, &["output_tokens", "completion_tokens", "output"])?
        },
        reasoning: if claude {
            optional_counter(v, "reasoning_output_tokens")?
        } else {
            aliased_counter(v, &["reasoning_output_tokens", "reasoning_tokens"])?
        },
        total: optional_counter(v, "total_tokens")?,
    };
    if !claude
        && result
            .cached
            .zip(result.input)
            .is_some_and(|(cached, input)| cached > input)
    {
        return Err("cached input exceeds inclusive input".into());
    }
    if result
        .reasoning
        .zip(result.output)
        .is_some_and(|(reasoning, output)| reasoning > output)
    {
        return Err("reasoning output exceeds inclusive output".into());
    }
    let mut raw = selected(
        v,
        &[
            "input_tokens",
            "output_tokens",
            cache_name,
            write_name,
            "reasoning_output_tokens",
            "total_tokens",
        ],
    );
    if !claude {
        for (name, value) in selected(
            v,
            &[
                "prompt_tokens",
                "input",
                "cache_read_input_tokens",
                "cached_tokens",
                "cache_creation_input_tokens",
                "completion_tokens",
                "output",
                "reasoning_tokens",
            ],
        )
        .as_object()
        .unwrap()
        {
            raw.as_object_mut()
                .unwrap()
                .insert(name.clone(), value.clone());
        }
    }
    if v.get("cache_creation").is_some() {
        raw.as_object_mut().unwrap().insert(
            "cache_creation".into(),
            if ttl.is_null() {
                Value::Null
            } else {
                selected(
                    ttl,
                    &["ephemeral_5m_input_tokens", "ephemeral_1h_input_tokens"],
                )
            },
        );
    }
    Ok((result, raw))
}

fn cache_writes(c: &CounterVector) -> Result<u64, String> {
    let split = c
        .cache_write_5m
        .unwrap_or(0)
        .checked_add(c.cache_write_1h.unwrap_or(0))
        .ok_or("cache creation counters overflow")?;
    if c.cache_write.is_some_and(|total| split > total) {
        return Err("cache creation duration split exceeds total".into());
    }
    Ok(c.cache_write.unwrap_or(split))
}

fn normalized(c: &CounterVector, claude: bool) -> Result<Tokens, String> {
    let input = c
        .input
        .ok_or_else(|| "missing input_tokens counter".to_owned())?;
    let output = c
        .output
        .ok_or_else(|| "missing output_tokens counter".to_owned())?;
    let cached = c.cached.unwrap_or(0);
    let writes = cache_writes(c)?;
    let uncached = if claude {
        input.checked_add(writes)
    } else {
        input.checked_sub(cached)
    }
    .ok_or_else(|| "invalid input token accounting".to_owned())?;
    let total = uncached
        .checked_add(cached)
        .and_then(|n| n.checked_add(output));
    if total.is_none_or(|n| n > i64::MAX as u64) {
        return Err("normalized token total overflows storage".into());
    }
    Ok(Tokens {
        uncached,
        cached,
        output,
    })
}

struct RecordContext<'a> {
    model: Option<&'a str>,
    tier: Option<&'a str>,
    account: Option<&'a str>,
    source: &'a str,
    weak: bool,
}

fn record(
    key: &str,
    raw_ts: &str,
    usage: &CounterVector,
    context: RecordContext<'_>,
) -> Result<Record, String> {
    let RecordContext {
        model,
        tier,
        account,
        source,
        weak,
    } = context;
    let ts = parse_timestamp(raw_ts).ok_or_else(|| "invalid metering timestamp".to_owned())?;
    let tokens = normalized(usage, source == "claude")?;
    let model = model.unwrap_or("");
    let meta = (source == "codex" && !is_deepseek(model)).then(|| metering::EventMeta {
        model: model.to_ascii_lowercase(),
        tier: tier.map(str::to_owned),
        account: account.map(str::to_owned),
        cache_write: cache_writes(usage).unwrap_or(0),
        uncertain_identity: weak,
    });
    let id = if source == "claude" {
        RecordId::Claude {
            message_id: key.into(),
        }
    } else {
        RecordId::Codex {
            identity: key.into(),
            ts: 0,
            uncached: tokens.uncached,
            cached: tokens.cached,
            output: tokens.output,
        }
    };
    Ok(Record {
        codex: meta,
        ts,
        family: family_for(model),
        tokens,
        id,
    })
}

fn parse_claude(v: &Value, path: &Path, offset: u64) -> Result<Vec<Evidence>, String> {
    let Some(message) = v.get("message").filter(|m| m.is_object()) else {
        return Ok(Vec::new());
    };
    let Some(usage) = message.get("usage").filter(|u| !u.is_null()) else {
        return Ok(Vec::new());
    };
    let model = text(&message["model"]);
    if model.as_deref().is_some_and(|m| !is_deepseek(m)) {
        return Ok(Vec::new());
    }
    if model.is_none() {
        return Err("metering message has no model; source coverage is unknown".into());
    }
    let raw_ts =
        text(&v["timestamp"]).ok_or_else(|| "metering message has no timestamp".to_owned())?;
    let (usage, raw_usage) = counters(usage, true)?;
    let session = text(&v["sessionId"]);
    let message_id = text(&message["id"]);
    let request = text(&v["requestId"]).or_else(|| text(&message["request_id"]));
    let sidechain = v.get("isSidechain").and_then(Value::as_bool);
    let weak = message_id.is_none() || request.is_none() && session.is_none();
    let source_path = path.to_string_lossy().into_owned();
    let identity = if let (Some(message), Some(request)) = (&message_id, &request) {
        key(&[json!("claude"), json!(message), json!(request)])
    } else if let Some(message) = &message_id {
        key(&[
            json!("claude"),
            json!(session.as_deref().unwrap_or(&source_path)),
            json!(message),
        ])
    } else {
        key(&[
            json!("claude_anonymous"),
            json!(source_path),
            json!(raw_ts),
            json!(offset),
        ])
    };
    let mut facts = selected(
        v,
        &["timestamp", "sessionId", "requestId", "isSidechain", "cwd"],
    );
    facts.as_object_mut().unwrap().insert(
        "message".into(),
        selected(message, &["id", "request_id", "model"]),
    );
    facts
        .as_object_mut()
        .unwrap()
        .insert("usage".into(), raw_usage);
    facts
        .as_object_mut()
        .unwrap()
        .insert("source".into(), json!("claude"));
    if weak {
        facts
            .as_object_mut()
            .unwrap()
            .insert("file_origin".into(), json!(source_path));
        facts
            .as_object_mut()
            .unwrap()
            .insert("offset".into(), json!(offset));
    }
    let record = record(
        &identity,
        &raw_ts,
        &usage,
        RecordContext {
            model: model.as_deref(),
            tier: None,
            account: None,
            source: "claude",
            weak,
        },
    )?;
    let preference = record.tokens.total().min((1u64 << 60) - 1) as i64
        + if sidechain == Some(true) {
            0
        } else {
            1i64 << 60
        };
    Ok(vec![Evidence {
        key: identity,
        canonical: facts.to_string(),
        record: Some(record),
        candidate: None,
        source: "claude".into(),
        raw_ts,
        session,
        message: message_id,
        request,
        turn: None,
        response: None,
        related_response: None,
        related_key: None,
        source_path,
        offset,
        kind: "message_usage".into(),
        issue: weak.then(|| "weak usage identity; copied records may be duplicated".into()),
        preference,
        sidechain,
        parent_session: None,
        fork_ts: None,
        weak,
        cumulative: None,
    }])
}

fn timestamp_order(raw: &str) -> Option<(i64, u32)> {
    let seconds = parse_timestamp(raw)?;
    let fractional = raw
        .split_once('.')
        .map(|(_, tail)| {
            tail.chars()
                .take_while(char::is_ascii_digit)
                .take(9)
                .collect::<String>()
        })
        .unwrap_or_default();
    let nanos = if fractional.is_empty() {
        0
    } else {
        fractional
            .parse::<u32>()
            .ok()?
            .checked_mul(10u32.pow(9 - fractional.len() as u32))?
    };
    Some((seconds, nanos))
}

fn context_field(c: &Context, name: &str) -> Option<String> {
    c.fields.get(name).and_then(text)
}
fn update_context(c: &mut Context, v: &Value, fields: &[&str]) -> Result<(), String> {
    let mut invalid = None;
    for field in fields {
        if let Some(value) = v.get(*field) {
            if value.is_string() || value.is_null() {
                c.fields.insert((*field).into(), value.clone());
            } else {
                // A changed protocol must not silently inherit stale pricing or
                // identity context. Keep only an unknown value, never its payload.
                c.fields.insert((*field).into(), Value::Null);
                invalid = Some(format!("invalid {field} context field type"));
            }
        }
    }
    invalid.map_or(Ok(()), Err)
}

#[derive(PartialEq)]
enum Movement {
    First,
    Repeat,
    Advance,
    Reset,
    Ambiguous,
}
fn movement(previous: Option<&CounterVector>, current: &CounterVector) -> Movement {
    let Some(previous) = previous else {
        return Movement::First;
    };
    if previous == current {
        return Movement::Repeat;
    }
    if [
        previous.input.zip(Some(current.input)),
        previous.output.zip(Some(current.output)),
        previous.cached.zip(Some(current.cached)),
        previous.total.zip(Some(current.total)),
    ]
    .iter()
    .flatten()
    .any(|(_, after)| after.is_none())
    {
        return Movement::Ambiguous;
    }
    let pairs = [
        previous.input.zip(current.input),
        previous.output.zip(current.output),
        previous.cached.zip(current.cached),
        previous.cache_write.zip(current.cache_write),
        previous.reasoning.zip(current.reasoning),
        previous.total.zip(current.total),
    ];
    if pairs
        .iter()
        .flatten()
        .all(|(before, after)| after >= before)
    {
        return Movement::Advance;
    }
    let reset = previous
        .total
        .zip(current.total)
        .is_some_and(|(before, after)| after < before)
        && [
            previous.input.zip(current.input),
            previous.output.zip(current.output),
        ]
        .iter()
        .flatten()
        .all(|(before, after)| after <= before);
    if reset {
        Movement::Reset
    } else {
        Movement::Ambiguous
    }
}

fn difference(
    current: &CounterVector,
    previous: Option<&CounterVector>,
) -> Result<CounterVector, String> {
    let subtract = |now: Option<u64>, before: Option<u64>| -> Result<Option<u64>, String> {
        match (now, before) {
            (Some(now), Some(before)) => now
                .checked_sub(before)
                .map(Some)
                .ok_or_else(|| "negative cumulative delta".into()),
            (now, None) => Ok(now),
            (None, Some(_)) => Err("cumulative counter disappeared".into()),
        }
    };
    let empty = CounterVector::default();
    let before = previous.unwrap_or(&empty);
    Ok(CounterVector {
        input: subtract(current.input, before.input)?,
        cached: subtract(current.cached, before.cached)?,
        cache_write: subtract(current.cache_write, before.cache_write)?,
        cache_write_5m: subtract(current.cache_write_5m, before.cache_write_5m)?,
        cache_write_1h: subtract(current.cache_write_1h, before.cache_write_1h)?,
        output: subtract(current.output, before.output)?,
        reasoning: subtract(current.reasoning, before.reasoning)?,
        total: subtract(current.total, before.total)?,
    })
}

fn same_counters(a: &CounterVector, b: &CounterVector) -> bool {
    a.input == b.input
        && a.output == b.output
        && a.cached.unwrap_or(0) == b.cached.unwrap_or(0)
        && a.cache_write.unwrap_or(0) == b.cache_write.unwrap_or(0)
}

fn parse_codex(
    v: &Value,
    path: &Path,
    offset: u64,
    c: &mut Context,
) -> Result<Vec<Evidence>, String> {
    let kind = v["type"].as_str().unwrap_or("");
    let p = &v["payload"];
    match kind {
        "session_meta" => {
            // Fork rollouts embed their ancestors' session_meta records. The
            // first header describes this file; later foreign headers must not
            // change its identity or move its fork boundary into the past.
            if context_field(c, "id")
                .zip(text(&p["id"]))
                .is_some_and(|(primary, incoming)| primary != incoming)
            {
                return Ok(Vec::new());
            }
            update_context(
                c,
                p,
                &[
                    "id",
                    "cwd",
                    "creator_account_id",
                    "forked_from_id",
                    "parent_thread_id",
                    "parent_session_id",
                ],
            )?;
            if !c.fields.contains_key("session_timestamp")
                && let Some(ts) = v.get("timestamp").filter(|v| v.is_string() || v.is_null())
            {
                c.fields.insert("session_timestamp".into(), ts.clone());
            }
            return Ok(Vec::new());
        }
        "turn_context" => {
            c.response = None;
            update_context(c, p, &["model", "turn_id", "service_tier", "cwd"])?;
            return Ok(Vec::new());
        }
        "event_msg" if p["type"] == "thread_settings_applied" => {
            update_context(c, &p["thread_settings"], &["model", "service_tier"])?;
            return Ok(Vec::new());
        }
        _ => {}
    }
    let kind = if kind == "event_msg" && p["type"] == "token_count" {
        "token_count"
    } else if ["token_usage_record", "compacted"].contains(&kind) {
        kind
    } else {
        return Ok(Vec::new());
    };
    let raw_ts =
        text(&v["timestamp"]).ok_or_else(|| "metering event has no timestamp".to_owned())?;
    parse_timestamp(&raw_ts).ok_or_else(|| "invalid metering timestamp".to_owned())?;
    for (field, value) in [
        ("model", p.get("model")),
        ("info.model", p.pointer("/info/model")),
        ("service_tier", p.get("service_tier")),
        ("thread_id", p.get("thread_id")),
        ("session_id", p.get("session_id")),
        ("turn_id", p.get("turn_id")),
        ("root_turn_id", p.get("root_turn_id")),
    ] {
        if value.is_some_and(|v| !v.is_null() && !v.is_string()) {
            return Err(format!("invalid metering {field} field type"));
        }
    }
    let source_path = path.to_string_lossy().into_owned();
    let mut session = context_field(c, "id");
    let turn = context_field(c, "turn_id");
    let model = text(&p["model"])
        .or_else(|| text(&p["info"]["model"]))
        .or_else(|| context_field(c, "model"));
    let tier = p
        .get("service_tier")
        .map(text)
        .unwrap_or_else(|| context_field(c, "service_tier"));
    let account = context_field(c, "creator_account_id");
    let response = if kind == "compacted" {
        text(&p["compaction_response_id"])
    } else {
        text(&p["response_id"])
    };
    let parent_session = context_field(c, "forked_from_id")
        .or_else(|| context_field(c, "parent_thread_id"))
        .or_else(|| context_field(c, "parent_session_id"));
    let fork_ts = parent_session
        .as_ref()
        .and_then(|_| context_field(c, "session_timestamp"));
    let mut facts =
        json!({"source":"codex","kind":kind,"timestamp":v["timestamp"],"context":c.fields});
    let map = facts.as_object_mut().unwrap();
    if let Some(value) = p
        .get("response_id")
        .filter(|v| v.is_string() || v.is_null())
    {
        map.insert("response_id".into(), value.clone());
    }
    if let Some(value) = p
        .get("compaction_response_id")
        .filter(|v| v.is_string() || v.is_null())
    {
        map.insert("compaction_response_id".into(), value.clone());
    }
    for field in ["model", "service_tier"] {
        if let Some(value) = p.get(field).filter(|v| v.is_string() || v.is_null()) {
            map.insert(format!("payload_{field}"), value.clone());
        }
    }
    if let Some(value) = p
        .pointer("/info/model")
        .filter(|v| v.is_string() || v.is_null())
    {
        map.insert("info_model".into(), value.clone());
    }
    let mut issue = None;
    let mut candidate = None;
    let mut countable = None;
    let mut related_response = None;
    let mut related_key = None;
    let mut cumulative = None;
    let mut paired_session = None;
    let mut weak = session.is_none();
    let identity;
    if kind == "compacted" {
        let id = response
            .as_deref()
            .ok_or_else(|| "compaction marker missing response identity".to_owned())?;
        if let Some(response) = c.response.as_ref().filter(|response| response.id == id) {
            session = response.session.clone().or(session);
        }
        identity = response_key(session.as_deref(), id, &source_path);
    } else if kind == "token_usage_record" {
        let id = response
            .as_deref()
            .ok_or_else(|| "response usage missing response identity".to_owned())?;
        for field in ["session_id", "thread_id", "turn_id", "root_turn_id"] {
            if let Some(value) = p.get(field).filter(|v| v.is_string() || v.is_null()) {
                map.insert(format!("payload_{field}"), value.clone());
            }
        }
        let local_response_key = response_key(session.as_deref(), id, &source_path);
        session = text(&p["thread_id"])
            .or_else(|| text(&p["session_id"]))
            .or(session);
        weak = session.is_none();
        let (usage, raw) = counters(&p["usage"], false)?;
        map.insert("usage".into(), raw);
        if let Some(raw) = p.get("turn_token_usage") {
            let raw = if raw.is_null() {
                Value::Null
            } else {
                counters(raw, false)?.1
            };
            map.insert("turn_token_usage".into(), raw);
        }
        let thread_usage = match p.get("thread_token_usage") {
            Some(Value::Null) => {
                map.insert("thread_token_usage".into(), Value::Null);
                None
            }
            Some(raw) => {
                let (counter, raw) = counters(raw, false)?;
                map.insert("thread_token_usage".into(), raw);
                Some(counter)
            }
            None => None,
        };
        let response_model = text(&p["model"]).or(model.clone());
        if let Some(raw) = p.get("model").filter(|v| v.is_string() || v.is_null()) {
            map.insert("model".into(), raw.clone());
        }
        identity = response_key(session.as_deref(), id, &source_path);
        candidate = Some(record(
            &identity,
            &raw_ts,
            &usage,
            RecordContext {
                model: response_model.as_deref(),
                tier: tier.as_deref(),
                account: account.as_deref(),
                source: "codex",
                weak,
            },
        )?);
        if turn.is_some()
            && c.last_turn == turn
            && thread_usage
                .as_ref()
                .zip(c.total.as_ref())
                .is_some_and(|(a, b)| a == b)
            && c.last_usage
                .as_ref()
                .is_some_and(|last| same_counters(last, &usage))
        {
            related_key = c.last_key.clone();
        }
        if related_key.is_none() && local_response_key != identity {
            related_key = Some(local_response_key);
        }
        c.response = Some(ResponseContext {
            id: id.to_owned(),
            session: session.clone(),
            usage,
            thread_usage,
            turn: turn.clone(),
        });
    } else {
        let info = &p["info"];
        if !info.is_object() {
            return Ok(Vec::new());
        } // Quota-only notifications have no usage.
        for field in ["last_token_usage", "total_token_usage"] {
            if info.get(field) == Some(&Value::Null) {
                map.insert(field.into(), Value::Null);
            }
        }
        let last = if let Some(raw) = info.get("last_token_usage").filter(|v| !v.is_null()) {
            let (counter, raw) = counters(raw, false)?;
            map.insert("last_token_usage".into(), raw);
            Some(counter)
        } else {
            None
        };
        cumulative = if let Some(raw) = info.get("total_token_usage").filter(|v| !v.is_null()) {
            let (counter, raw) = counters(raw, false)?;
            map.insert("total_token_usage".into(), raw);
            Some(counter)
        } else {
            None
        };
        if last.is_none() && cumulative.is_none() {
            return Ok(Vec::new());
        }
        let movement = cumulative.as_ref().map_or(Movement::Ambiguous, |current| {
            movement(c.total.as_ref(), current)
        });
        if movement == Movement::Reset {
            c.epoch = c
                .epoch
                .checked_add(1)
                .ok_or("counter reset epoch overflow")?;
        }
        c.ordinal = c
            .ordinal
            .checked_add(1)
            .ok_or("source event ordinal overflow")?;
        weak |= turn.is_none() || cumulative.is_none() || movement == Movement::Ambiguous;
        map.insert("reset_epoch".into(), json!(c.epoch));
        identity = if movement == Movement::Repeat {
            c.last_key
                .clone()
                .unwrap_or_else(|| key(&[json!("codex_repeat"), json!(source_path), json!(offset)]))
        } else if let Some(total) = &cumulative {
            if movement == Movement::Ambiguous {
                key(&[
                    json!("codex_ambiguous"),
                    json!(session.as_deref().unwrap_or(&source_path)),
                    json!(turn),
                    json!(raw_ts),
                    json!(offset),
                ])
            } else {
                key(&[
                    json!("codex_count"),
                    json!(session.as_deref().unwrap_or(&source_path)),
                    json!(turn),
                    json!(c.epoch),
                    json!(total),
                ])
            }
        } else {
            key(&[
                json!("codex_weak"),
                json!(session.as_deref().unwrap_or(&source_path)),
                json!(turn),
                json!(raw_ts),
                json!(offset),
            ])
        };
        let usage = if movement == Movement::Repeat {
            None
        } else if last.is_some() {
            last.clone()
        } else if movement == Movement::Ambiguous {
            None
        } else {
            Some(difference(
                cumulative.as_ref().unwrap(),
                if movement == Movement::Reset {
                    None
                } else {
                    c.total.as_ref()
                },
            )?)
        };
        if movement == Movement::Ambiguous && cumulative.is_some() {
            issue = Some("ambiguous cumulative counter reset; delta inference disabled".into());
        }
        let preceding_response = c.response.take();
        if let Some(usage) = &usage {
            if let Some(resp) = preceding_response
                && resp.turn == turn
                && (same_counters(&resp.usage, usage)
                    || resp
                        .thread_usage
                        .as_ref()
                        .zip(cumulative.as_ref())
                        .is_some_and(|(a, b)| a == b))
            {
                related_response = Some(resp.id);
                paired_session = resp.session;
            }
            countable = Some(record(
                &identity,
                &raw_ts,
                usage,
                RecordContext {
                    model: model.as_deref(),
                    tier: tier.as_deref(),
                    account: account.as_deref(),
                    source: "codex",
                    weak,
                },
            )?);
            if related_response.is_some() {
                c.response = None;
            }
            c.last_usage = Some(usage.clone());
            c.last_turn = turn.clone();
            c.last_key = Some(identity.clone());
        }
        if let Some(total) = &cumulative {
            c.total = Some(total.clone());
        }
    }
    if model.is_none() && kind != "compacted" && issue.is_none() {
        issue = Some("metering model is missing; usage is retained unpriced".into());
    }
    if weak {
        map.insert("file_origin".into(), json!(source_path));
        map.insert("offset".into(), json!(offset));
        if issue.is_none() {
            issue = Some("weak usage identity; copied records may be duplicated".into());
        }
    }
    // A preserved, explicitly pre-fork cumulative event has the same conditional
    // identity as its parent. Rewritten timestamps stay unresolved rather than
    // matching an unrelated equal token tuple.
    if related_key.is_none()
        && kind == "token_count"
        && !weak
        && let (Some(parent), Some(boundary), Some(total)) =
            (&parent_session, &fork_ts, &cumulative)
        && timestamp_order(&raw_ts)
            .zip(timestamp_order(boundary))
            .is_some_and(|(event, boundary)| event <= boundary)
    {
        related_key = Some(key(&[
            json!("codex_count"),
            json!(parent),
            json!(turn),
            json!(c.epoch),
            json!(total),
        ]));
    }
    if kind != "compacted" && parent_session.is_some() && related_key.is_none() && issue.is_none() {
        let position = fork_ts
            .as_deref()
            .and_then(timestamp_order)
            .zip(timestamp_order(&raw_ts));
        let unresolved = position.is_none_or(|(boundary, event)| event <= boundary);
        if unresolved && !c.fork_warning_emitted {
            issue = Some(
                "fork replay provenance is incomplete; copied history may be duplicated".into(),
            );
            c.fork_warning_emitted = true;
        }
    }
    let mut identity = identity;
    if let Some(response) = &related_response {
        related_key = Some(identity.clone());
        session = paired_session.or(session);
        identity = response_key(session.as_deref(), response, &source_path);
        if let Some(record) = &mut countable {
            set_record_key(record, &identity);
        }
        c.last_key = Some(identity.clone());
    }
    Ok(vec![Evidence {
        key: identity,
        canonical: facts.to_string(),
        record: countable,
        candidate,
        source: "codex".into(),
        raw_ts,
        session,
        message: None,
        request: None,
        turn,
        response,
        related_response,
        related_key,
        source_path,
        offset,
        kind: kind.into(),
        issue,
        preference: 0,
        sidechain: None,
        parent_session,
        fork_ts,
        weak,
        cumulative,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;
    fn codex(v: Value, offset: u64, c: &mut Context) -> Vec<Evidence> {
        parse_line(
            Source::Codex,
            &v.to_string(),
            Path::new("/source/a.jsonl"),
            offset,
            c,
        )
        .unwrap()
    }
    fn context() -> Context {
        let mut c = Context::default();
        codex(
            json!({"type":"session_meta","timestamp":"2026-10-26T00:00:00Z","payload":{"id":"s","cwd":"/work","creator_account_id":"a","unrelated_secret":"never"}}),
            0,
            &mut c,
        );
        codex(
            json!({"type":"turn_context","payload":{"turn_id":"t","model":"gpt-6.1-sol","service_tier":"standard"}}),
            1,
            &mut c,
        );
        c
    }
    fn usage(input: u64, output: u64) -> Value {
        json!({"input_tokens":input,"cached_input_tokens":0,"output_tokens":output,"total_tokens":input+output})
    }
    fn count(last: Value, total: Value) -> Value {
        json!({"type":"event_msg","timestamp":"2026-10-26T01:00:00.123Z","payload":{"type":"token_count","info":{"last_token_usage":last,"total_token_usage":total}}})
    }
    fn response() -> Value {
        json!({"type":"token_usage_record","timestamp":"2026-10-26T01:00:00.123Z","payload":{"response_id":"r","usage":usage(100,10),"thread_token_usage":usage(100,10)}})
    }
    fn marker() -> Value {
        json!({"type":"compacted","timestamp":"2026-10-26T01:00:00.124Z","payload":{"compaction_response_id":"r"}})
    }
    fn claude(message: &str, session: &str, output: u64) -> Value {
        json!({"timestamp":"2026-10-26T01:00:00.123Z","sessionId":session,"cwd":"/work","message":{"id":message,"model":"deepseek-v4-pro","usage":{"input_tokens":10,"cache_read_input_tokens":20,"cache_creation_input_tokens":5,"output_tokens":output},"content":[{"text":"private prompt"}]},"toolUseResult":"secret"})
    }
    fn cl(v: Value) -> Evidence {
        parse_line(
            Source::Claude,
            &v.to_string(),
            Path::new("/source/c.jsonl"),
            0,
            &mut Context::default(),
        )
        .unwrap()
        .remove(0)
    }

    #[test]
    fn allowlist_keeps_needed_fields_without_content() {
        let e = cl(claude("m", "s", 3));
        assert!(e.canonical.contains("deepseek-v4-pro"));
        assert!(!e.canonical.contains("private prompt"));
        assert!(!e.canonical.contains("secret"));
        let t = e.record.unwrap().tokens;
        assert_eq!((t.uncached, t.cached, t.output), (15, 20, 3));
    }
    #[test]
    fn claude_namespace_and_streaming_select_complete_record() {
        let a = cl(claude("m", "s1", 3));
        let b = cl(claude("m", "s2", 3));
        assert_ne!(a.key, b.key);
        let larger = cl(claude("m", "s1", 6));
        let selected = reconcile(&[a, larger]).unwrap();
        assert_eq!(selected.record.unwrap().tokens.output, 6);
    }
    #[test]
    fn model_and_optional_context_changes_preserve_identity_but_not_equality() {
        let mut a = claude("m", "s", 3);
        let e1 = cl(a.clone());
        a["message"]["model"] = json!("deepseek-flash");
        a["cwd"] = Value::Null;
        let e2 = cl(a);
        assert_eq!(e1.key, e2.key);
        assert_ne!(e1.canonical, e2.canonical);
    }
    #[test]
    fn missing_null_and_zero_counters_are_distinct_evidence() {
        let a = claude("m", "s", 3);
        let mut b = a.clone();
        b["message"]["usage"]["cache_creation_input_tokens"] = Value::Null;
        let mut c = a.clone();
        c["message"]["usage"]
            .as_object_mut()
            .unwrap()
            .remove("cache_creation_input_tokens");
        let mut d = a.clone();
        d["message"]["usage"]["cache_creation_input_tokens"] = json!(0);
        let c1 = cl(b).canonical;
        let c2 = cl(c).canonical;
        let c3 = cl(d).canonical;
        assert_ne!(c1, c2);
        assert_ne!(c2, c3);
        assert_ne!(c1, c3);
    }
    #[test]
    fn same_cumulative_update_is_not_a_new_request_across_restart() {
        let mut c = context();
        let first = codex(count(usage(100, 10), usage(100, 10)), 10, &mut c).remove(0);
        let mut restored: Context =
            serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        let repeated = codex(count(usage(100, 10), usage(100, 10)), 20, &mut restored).remove(0);
        assert_eq!(first.key, repeated.key);
        assert!(repeated.record.is_none());
    }
    #[test]
    fn inferred_reset_does_not_collapse_a_later_equal_vector() {
        let mut c = context();
        let first = codex(count(usage(100, 10), usage(100, 10)), 10, &mut c).remove(0);
        codex(count(usage(100, 10), usage(200, 20)), 20, &mut c);
        codex(count(usage(50, 5), usage(50, 5)), 30, &mut c);
        let after = codex(count(usage(50, 5), usage(100, 10)), 40, &mut c).remove(0);
        assert_ne!(first.key, after.key);
    }
    #[test]
    fn mixed_regression_never_invents_a_delta() {
        let mut c = context();
        codex(count(usage(100, 10), usage(100, 10)), 10, &mut c);
        let v = count(Value::Null, usage(200, 5));
        let e = codex(v, 20, &mut c).remove(0);
        assert!(e.record.is_none());
        assert!(e.weak);
        assert!(e.issue.unwrap().contains("ambiguous"));
    }
    #[test]
    fn cumulative_only_uses_checked_difference() {
        let mut c = context();
        codex(count(usage(100, 10), usage(100, 10)), 10, &mut c);
        let e = codex(count(Value::Null, usage(150, 15)), 20, &mut c).remove(0);
        let t = e.record.unwrap().tokens;
        assert_eq!((t.uncached, t.output), (50, 5));
    }
    #[test]
    fn delayed_compaction_and_count_converge_across_restart() {
        let mut c = context();
        let r = codex(response(), 10, &mut c).remove(0);
        assert!(r.record.is_none());
        let m = codex(marker(), 20, &mut c).remove(0);
        let first = reconcile(&[r.clone(), m.clone()]).unwrap();
        assert_eq!(first.record.unwrap().tokens.total(), 110);
        let mut c: Context = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        let t = codex(count(usage(100, 10), usage(100, 10)), 30, &mut c).remove(0);
        assert_eq!(t.key, r.key);
        let s = reconcile(&[r, m, t]).unwrap();
        assert_eq!(s.record.unwrap().tokens.total(), 110);
        assert_eq!(s.selected, 2);
    }
    #[test]
    fn response_after_count_exposes_prior_event_alias() {
        let mut c = context();
        let t = codex(count(usage(100, 10), usage(100, 10)), 10, &mut c).remove(0);
        let m = codex(marker(), 20, &mut c).remove(0);
        let r = codex(response(), 30, &mut c).remove(0);
        assert_eq!(r.related_key.as_deref(), Some(t.key.as_str()));
        let s = reconcile(&[t, m, r]).unwrap();
        assert_eq!(s.record.unwrap().tokens.total(), 110);
    }
    #[test]
    fn equal_unrelated_usage_after_consumed_response_is_not_linked() {
        let mut c = context();
        codex(response(), 10, &mut c);
        let first = codex(count(usage(100, 10), usage(100, 10)), 20, &mut c).remove(0);
        assert!(first.related_response.is_some());
        let later = codex(count(usage(100, 10), usage(200, 20)), 30, &mut c).remove(0);
        assert!(later.related_response.is_none());
        assert_ne!(first.key, later.key);
    }
    #[test]
    fn unsupported_fields_do_not_archive_nested_content() {
        let mut v = claude("m", "s", 3);
        v["cwd"] = json!({"secret":"forbidden"});
        let e = cl(v);
        assert!(!e.canonical.contains("forbidden"));
    }
    #[test]
    fn malformed_counters_are_errors_without_source_echo() {
        let mut v = claude("m", "s", 3);
        v["message"]["usage"]["output_tokens"] = json!("private prompt");
        let error = parse_line(
            Source::Claude,
            &v.to_string(),
            Path::new("/a"),
            0,
            &mut Context::default(),
        )
        .unwrap_err();
        assert!(!error.contains("private prompt"));
    }
    #[test]
    fn missing_tier_preserves_prior_settings_and_model_correction_key() {
        let mut c = context();
        let a = codex(count(usage(100, 10), usage(100, 10)), 10, &mut c).remove(0);
        codex(
            json!({"type":"event_msg","payload":{"type":"thread_settings_applied","thread_settings":{"model":"gpt-6-astra"}}}),
            20,
            &mut c,
        );
        assert_eq!(
            context_field(&c, "service_tier").as_deref(),
            Some("standard")
        );
        let b = codex(count(usage(100, 10), usage(100, 10)), 30, &mut c).remove(0);
        assert_eq!(a.key, b.key);
        assert_ne!(a.canonical, b.canonical);
    }
    #[test]
    fn empty_context_checkpoint_and_optional_counter_disappearance() {
        let c: Context = serde_json::from_str("{}").unwrap();
        assert!(c.total.is_none());
        let mut c = context();
        codex(count(usage(100, 10), usage(100, 10)), 10, &mut c);
        let mut total = usage(200, 20);
        total.as_object_mut().unwrap().remove("cached_input_tokens");
        let e = codex(count(Value::Null, total), 20, &mut c).remove(0);
        assert!(e.record.is_none());
        assert!(e.weak);
    }
    #[test]
    fn fork_prefix_exposes_parent_alias_but_new_calls_do_not() {
        let mut parent = context();
        let p = codex(count(usage(100, 10), usage(100, 10)), 10, &mut parent).remove(0);
        let mut child = Context::default();
        codex(
            json!({"type":"session_meta","timestamp":"2026-10-26T01:00:01Z","payload":{"id":"child","forked_from_id":"s"}}),
            0,
            &mut child,
        );
        codex(
            json!({"type":"turn_context","payload":{"turn_id":"t","model":"gpt-6.1-sol"}}),
            1,
            &mut child,
        );
        let copied = codex(count(usage(100, 10), usage(100, 10)), 10, &mut child).remove(0);
        assert_eq!(copied.related_key.as_deref(), Some(p.key.as_str()));
        let mut event = count(usage(100, 10), usage(200, 20));
        event["timestamp"] = json!("2026-10-26T01:00:02Z");
        let fresh = codex(event, 20, &mut child).remove(0);
        assert!(fresh.related_key.is_none());
        assert!(fresh.issue.is_none());
    }
    #[test]
    fn unrelated_usage_and_different_thread_total_do_not_pair() {
        let mut c = context();
        codex(response(), 10, &mut c);
        let e = codex(count(usage(200, 20), usage(200, 20)), 20, &mut c).remove(0);
        assert!(e.related_response.is_none());
    }
    #[test]
    fn weak_anonymous_identity_is_source_local() {
        let mut v = claude("m", "s", 3);
        v["message"].as_object_mut().unwrap().remove("id");
        let a = parse_line(
            Source::Claude,
            &v.to_string(),
            Path::new("/a"),
            0,
            &mut Context::default(),
        )
        .unwrap()
        .remove(0);
        let b = parse_line(
            Source::Claude,
            &v.to_string(),
            Path::new("/b"),
            0,
            &mut Context::default(),
        )
        .unwrap()
        .remove(0);
        assert!(a.weak && b.weak);
        assert_ne!(a.key, b.key);
        assert_ne!(a.canonical, b.canonical);
    }
    #[test]
    fn codex_known_counter_aliases_are_retained_and_checked() {
        let mut c = context();
        let e=codex(count(json!({"prompt_tokens":100,"cached_tokens":20,"completion_tokens":10,"reasoning_tokens":3}),usage(100,10)),10,&mut c).remove(0);
        let t = e.record.unwrap().tokens;
        assert_eq!((t.uncached, t.cached, t.output), (80, 20, 10));
        assert!(e.canonical.contains("prompt_tokens"));
        let bad = count(
            json!({"input_tokens":100,"prompt_tokens":99,"output_tokens":10}),
            usage(200, 20),
        );
        assert!(parse_line(Source::Codex, &bad.to_string(), Path::new("/a"), 20, &mut c).is_err());
    }
    #[test]
    fn cache_creation_duration_counts_are_available_without_parent_total() {
        let mut v = claude("m", "s", 3);
        v["message"]["usage"]
            .as_object_mut()
            .unwrap()
            .remove("cache_creation_input_tokens");
        v["message"]["usage"]["cache_creation"] = json!({"ephemeral_5m_input_tokens":4,"ephemeral_1h_input_tokens":5,"private":"forbidden"});
        let e = cl(v);
        assert_eq!(e.record.unwrap().tokens.uncached, 19);
        assert!(!e.canonical.contains("forbidden"));
    }
    #[test]
    fn late_response_cannot_alias_prior_turn_with_equal_counters() {
        let mut c = context();
        let before = codex(count(usage(100, 10), usage(100, 10)), 10, &mut c).remove(0);
        let mut c: Context = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        codex(
            json!({"type":"turn_context","payload":{"turn_id":"next","model":"gpt-6.1-sol"}}),
            20,
            &mut c,
        );
        let response = codex(response(), 30, &mut c).remove(0);
        assert!(response.related_key.is_none());
        let marker = codex(marker(), 40, &mut c).remove(0);
        assert_ne!(before.key, response.key);
        assert!(reconcile(&[response, marker]).unwrap().record.is_some());
    }
    #[test]
    fn unresolved_fork_boundary_warns_once_across_restart() {
        let mut c = context();
        c.fields.insert("forked_from_id".into(), json!("parent"));
        c.fields.remove("session_timestamp");
        let first = codex(count(usage(100, 10), usage(100, 10)), 10, &mut c).remove(0);
        assert!(first.issue.as_deref().unwrap().contains("fork replay"));
        let mut c: Context = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        let next = codex(count(usage(100, 10), usage(200, 20)), 20, &mut c).remove(0);
        assert!(next.issue.is_none());
    }
    #[test]
    fn malformed_metadata_clears_stale_tier_without_echoing_payload() {
        let mut c = context();
        let bad = json!({"type":"event_msg","payload":{"type":"thread_settings_applied","thread_settings":{"service_tier":{"secret":"forbidden"}}}});
        let error =
            parse_line(Source::Codex, &bad.to_string(), Path::new("/a"), 0, &mut c).unwrap_err();
        assert!(!error.contains("forbidden"));
        assert!(context_field(&c, "service_tier").is_none());
        let event = codex(count(usage(100, 10), usage(100, 10)), 10, &mut c).remove(0);
        assert!(event.record.unwrap().codex.unwrap().tier.is_none());
        assert!(!event.canonical.contains("forbidden"));
    }
    #[test]
    fn embedded_parent_metadata_does_not_replace_primary_fork_identity() {
        let mut c = Context::default();
        codex(
            json!({"type":"session_meta","timestamp":"2026-10-26T02:00:00Z","payload":{"id":"child","forked_from_id":"parent"}}),
            0,
            &mut c,
        );
        codex(
            json!({"type":"session_meta","timestamp":"2026-10-25T02:00:00Z","payload":{"id":"parent","forked_from_id":"ancestor"}}),
            1,
            &mut c,
        );
        codex(
            json!({"type":"session_meta","timestamp":"2026-10-24T02:00:00Z","payload":{"id":"ancestor"}}),
            2,
            &mut c,
        );
        codex(
            json!({"type":"turn_context","payload":{"turn_id":"t","model":"gpt-6.1-sol"}}),
            3,
            &mut c,
        );
        let e = codex(count(usage(100, 10), usage(100, 10)), 10, &mut c).remove(0);
        assert_eq!(e.session.as_deref(), Some("child"));
        assert_eq!(e.parent_session.as_deref(), Some("parent"));
        assert_eq!(e.fork_ts.as_deref(), Some("2026-10-26T02:00:00Z"));
        assert_eq!(
            serde_json::from_str::<Value>(e.related_key.as_ref().unwrap()).unwrap()[1],
            json!("parent")
        );
    }
    #[test]
    fn adjacent_response_is_linked_when_thread_and_legacy_totals_have_different_scopes() {
        let mut c = context();
        let mut r = response();
        r["payload"]["thread_token_usage"] = usage(1000, 100);
        let r = codex(r, 10, &mut c).remove(0);
        let t = codex(count(usage(100, 10), usage(100, 10)), 20, &mut c).remove(0);
        assert_eq!(t.related_response.as_deref(), Some("r"));
        assert_eq!(r.key, t.key);
    }
    #[test]
    fn first_unmatched_count_consumes_response_context() {
        let mut c = context();
        let mut r = response();
        r["payload"]
            .as_object_mut()
            .unwrap()
            .remove("thread_token_usage");
        codex(r, 10, &mut c);
        let unrelated = codex(count(usage(50, 5), usage(50, 5)), 20, &mut c).remove(0);
        assert!(unrelated.related_response.is_none());
        let later = codex(count(usage(100, 10), usage(150, 15)), 30, &mut c).remove(0);
        assert!(later.related_response.is_none());
    }
    #[test]
    fn explicit_response_owner_survives_copied_source_and_early_marker() {
        let mut c = context();
        c.fields.insert("id".into(), json!("child"));
        let marker = codex(marker(), 10, &mut c).remove(0);
        let mut raw = response();
        raw["payload"]["thread_id"] = json!("parent");
        raw["payload"]["session_id"] = json!("root");
        raw["payload"]["turn_id"] = json!("t");
        raw["payload"]["root_turn_id"] = json!("root-turn");
        let response = codex(raw, 20, &mut c).remove(0);
        assert_eq!(response.session.as_deref(), Some("parent"));
        assert_eq!(response.related_key.as_deref(), Some(marker.key.as_str()));
        assert!(response.canonical.contains("payload_root_turn_id"));
        let selected = reconcile(&[marker, response.clone()]).unwrap();
        assert_eq!(selected.key, response.key);
        assert!(selected.record.is_some());
        let count = codex(count(usage(100, 10), usage(100, 10)), 30, &mut c).remove(0);
        assert_eq!(count.key, response.key);
        assert_eq!(count.session.as_deref(), Some("parent"));
    }
    #[test]
    fn null_response_context_is_retained_distinctly_from_missing() {
        let mut absent = response();
        absent["payload"]
            .as_object_mut()
            .unwrap()
            .remove("thread_token_usage");
        let mut explicit = absent.clone();
        explicit["payload"]["thread_token_usage"] = Value::Null;
        explicit["payload"]["turn_token_usage"] = Value::Null;
        let a = codex(absent, 10, &mut context()).remove(0);
        let b = codex(explicit, 10, &mut context()).remove(0);
        assert_ne!(a.canonical, b.canonical);
        let facts: Value = serde_json::from_str(&b.canonical).unwrap();
        assert_eq!(facts.get("thread_token_usage"), Some(&Value::Null));
        assert_eq!(facts.get("turn_token_usage"), Some(&Value::Null));
    }
    #[test]
    fn malformed_per_event_model_and_tier_are_visible_without_payload_echo() {
        for field in ["model", "service_tier"] {
            let mut event = count(usage(100, 10), usage(100, 10));
            event["payload"][field] = json!({"secret":"forbidden"});
            let error = parse_line(
                Source::Codex,
                &event.to_string(),
                Path::new("/a"),
                0,
                &mut context(),
            )
            .unwrap_err();
            assert!(error.contains(field));
            assert!(!error.contains("forbidden"));
        }
    }
    #[test]
    fn compaction_marker_does_not_report_missing_usage_provenance() {
        let mut c = context();
        c.fields.insert("forked_from_id".into(), json!("parent"));
        c.fields
            .insert("session_timestamp".into(), json!("2026-10-26T02:00:00Z"));
        let marker = codex(marker(), 10, &mut c).remove(0);
        assert!(marker.issue.is_none());
        assert!(!c.fork_warning_emitted);
        c.fields.remove("session_timestamp");
        let unresolved = codex(count(usage(100, 10), usage(100, 10)), 20, &mut c).remove(0);
        assert!(unresolved.issue.unwrap().contains("fork replay"));
    }
}
