//! Assistant task pane: an in-app spreadsheet agent in the spirit of Claude for Excel.
//!
//! With Claude, the model works on the open workbook through the same tools as GridCraft's MCP
//! server (read ranges, write values, format, tables, charts, any command), executed in-process
//! on the UI thread. The app layer registers those tools with [`register_tools`] (this crate may
//! not depend on `gridcraft-mcp`). Answers stream in, cell references like `[[B4]]` become links,
//! edits that would overwrite data ask first, and everything one answer changes is one undo step.
//! DeepSeek runs the same agent loop over its OpenAI-style API. Gemini remains available as a
//! plain chat that proposes formulas and cell blocks.
//!
//! The user's own API keys live in the settings folder (`assistant.json`); requests run on a
//! background thread and are only sent when the user asks something.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};

use egui::{Color32, Ui};
use gridcraft_engine::core::{CellRef, RangeRef};
use gridcraft_engine::formula::FormulaLocale;
use gridcraft_engine::model::Workbook;
use serde_json::{Value as Json, json};

use crate::SheetApp;
use crate::theme::{self, Tokens};

// ------------------------------------------------------------------ tools from the app layer

/// The tool catalog (MCP-style `{name, description, inputSchema}` objects).
pub type ToolDefinitions = fn() -> Vec<Json>;
/// Runs one tool against a control-channel caller; returns `{content: [{type, text}], isError}`.
pub type ToolCaller = fn(&mut dyn FnMut(&str, Json) -> Result<Json, String>, &str, &Json) -> Json;

static TOOLS: std::sync::OnceLock<(ToolDefinitions, ToolCaller)> = std::sync::OnceLock::new();

/// Called once by the desktop app with `gridcraft_mcp`'s tools.
pub fn register_tools(definitions: ToolDefinitions, caller: ToolCaller) {
    let _ = TOOLS.set((definitions, caller));
}

/// Tools that need a window this pane cannot provide, or that only make sense for outside agents.
const HIDDEN_TOOLS: &[&str] = &["screenshot"];

// ------------------------------------------------------------------ settings

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Provider {
    Claude,
    DeepSeek,
    Gemini,
}

const CLAUDE_MODELS: &[(&str, &str, &str)] = &[
    ("claude-opus-5-5", "Claude Opus 5.5 — o mais capaz", "Claude Opus 5.5 — most capable"),
    ("claude-sonnet-5-5", "Claude Sonnet 5.5 — rápido", "Claude Sonnet 5.5 — fast"),
    ("claude-haiku-5-5", "Claude Haiku 5.5 — o mais barato", "Claude Haiku 5.5 — cheapest"),
];
const EFFORTS: &[(&str, &str, &str)] =
    &[("low", "Rápido", "Fast"), ("medium", "Equilibrado", "Balanced"), ("high", "Caprichado", "Thorough")];
const GEMINI_DEFAULT_MODEL: &str = "gemini-3.8-flash";
/// Gemini models tried after the chosen one when it is overloaded or gone.
const GEMINI_FALLBACKS: &[&str] = &["gemini-3.6-flash", "gemini-3.5-flash-lite"];
const CLAUDE_KEY_URL: &str = "https://console.anthropic.com/settings/keys";
const GEMINI_KEY_URL: &str = "https://aistudio.google.com/apikey";
const DEEPSEEK_KEY_URL: &str = "https://platform.deepseek.com/api_keys";
const DEEPSEEK_MODELS: &[(&str, &str, &str)] = &[
    ("deepseek-flash", "DeepSeek Flash — rápido e barato", "DeepSeek Flash — fast and cheap"),
    ("deepseek-v4-pro", "DeepSeek V4 Pro — o mais capaz", "DeepSeek V4 Pro — most capable"),
];
/// Cap on the sheet preview sent with each question.
const MAX_CONTEXT_CHARS: usize = 12_000;
/// Model requests per answer before the agent stops (each tool round is one request).
const MAX_ROUNDS: usize = 40;

#[derive(Clone, Debug)]
struct Settings {
    provider: Provider,
    claude_key: String,
    claude_model: String,
    effort: String,
    gemini_key: String,
    gemini_model: String,
    deepseek_key: String,
    deepseek_model: String,
    instructions: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            provider: Provider::Claude,
            claude_key: String::new(),
            claude_model: CLAUDE_MODELS[0].0.into(),
            effort: "medium".into(),
            gemini_key: String::new(),
            gemini_model: GEMINI_DEFAULT_MODEL.into(),
            deepseek_key: String::new(),
            deepseek_model: DEEPSEEK_MODELS[0].0.into(),
            instructions: String::new(),
        }
    }
}

fn settings_dir() -> Option<std::path::PathBuf> {
    if std::env::var_os("GRIDCRAFT_NO_PREFS").is_some() {
        return None;
    }
    if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Library/Application Support/GridCraft"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| std::path::PathBuf::from(a).join("GridCraft"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
            .map(|c| c.join("gridcraft"))
    }
}

impl Settings {
    fn load() -> Settings {
        let mut s = Settings::default();
        let read = |name: &str| -> Option<Json> {
            settings_dir().and_then(|d| std::fs::read_to_string(d.join(name)).ok()).and_then(|t| serde_json::from_str(&t).ok())
        };
        if let Some(v) = read("assistant.json") {
            let text = |k: &str| v[k].as_str().map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
            s.provider = match v["provider"].as_str() {
                Some("gemini") => Provider::Gemini,
                Some("deepseek") => Provider::DeepSeek,
                _ => Provider::Claude,
            };
            s.claude_key = text("claudeKey").unwrap_or_default();
            s.claude_model = text("claudeModel").unwrap_or(s.claude_model);
            s.effort = text("effort").unwrap_or(s.effort);
            s.gemini_key = text("geminiKey").unwrap_or_default();
            s.gemini_model = text("geminiModel").unwrap_or(s.gemini_model);
            s.deepseek_key = text("deepseekKey").unwrap_or_default();
            s.deepseek_model = text("deepseekModel").unwrap_or(s.deepseek_model);
            s.instructions = v["instructions"].as_str().unwrap_or_default().to_string();
        } else if let Some(v) = read("gemini.json") {
            // The first version of this pane kept only a Gemini key.
            s.gemini_key = v["apiKey"].as_str().unwrap_or_default().trim().to_string();
        }
        if s.claude_key.is_empty() {
            s.claude_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_default().trim().to_string();
        }
        if s.deepseek_key.is_empty() {
            s.deepseek_key = std::env::var("DEEPSEEK_API_KEY").unwrap_or_default().trim().to_string();
        }
        if s.gemini_key.is_empty() {
            s.gemini_key = std::env::var("GEMINI_API_KEY").unwrap_or_default().trim().to_string();
        }
        s
    }

    fn save(&self) {
        let Some(dir) = settings_dir() else { return };
        let _ = std::fs::create_dir_all(&dir);
        let v = json!({
            "provider": match self.provider {
                Provider::Claude => "claude",
                Provider::DeepSeek => "deepseek",
                Provider::Gemini => "gemini",
            },
            "claudeKey": self.claude_key,
            "claudeModel": self.claude_model,
            "effort": self.effort,
            "geminiKey": self.gemini_key,
            "geminiModel": self.gemini_model,
            "deepseekKey": self.deepseek_key,
            "deepseekModel": self.deepseek_model,
            "instructions": self.instructions,
        });
        let _ = std::fs::write(dir.join("assistant.json"), serde_json::to_string_pretty(&v).unwrap_or_default());
    }

    fn key(&self) -> &str {
        match self.provider {
            Provider::Claude => &self.claude_key,
            Provider::DeepSeek => &self.deepseek_key,
            Provider::Gemini => &self.gemini_key,
        }
    }

    fn model(&self) -> &str {
        match self.provider {
            Provider::Claude => &self.claude_model,
            Provider::DeepSeek => &self.deepseek_model,
            Provider::Gemini => &self.gemini_model,
        }
    }
}

// ------------------------------------------------------------------ conversation state

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    User(String),
    /// Model prose (streamed); may hold code blocks and `[[A1]]` citations.
    Text(String),
    /// A short note the model wrote between tool calls.
    Progress(String),
    /// One tool the agent ran: a readable label and whether it worked (`None` while running).
    Tool { label: String, ok: Option<bool> },
    Error(String),
}

/// Messages from the worker thread.
enum Event {
    Text(String),
    Progress(String),
    ToolCall { name: String, input: Json, reply: Sender<(String, bool)> },
    /// The turn ended: the full Claude message history to keep (`None` for Gemini), or an error.
    Done(Result<Option<Vec<Json>>, String>),
}

/// A tool call waiting for the user's go-ahead.
struct Pending {
    name: String,
    input: Json,
    reply: Sender<(String, bool)>,
    question: String,
}

pub struct Assistant {
    pub items: Vec<Item>,
    pub input: String,
    settings: Settings,
    loaded: bool,
    show_settings: bool,
    /// Claude's API history (append-only: thinking blocks are passed back unchanged).
    history: Vec<Json>,
    /// DeepSeek's OpenAI-style history, kept whole: with tools its API requires every earlier
    /// `reasoning_content` back.
    ds_history: Vec<Json>,
    /// Who answers the running turn.
    turn_provider: Provider,
    rx: Option<Receiver<Event>>,
    stop: Arc<AtomicBool>,
    pending: Option<Pending>,
    /// Overwrites allowed without asking for the rest of this conversation.
    allow_all: bool,
    /// The workbook before the running answer: (doc uid, undo depth, snapshot).
    turn_start: Option<(u64, usize, Arc<Workbook>)>,
    /// The system prompt this Claude conversation started with. It never changes mid-conversation:
    /// every thinking block is bound to it, and a different one makes the API reject the history.
    conv_system: Option<String>,
    /// Standing instructions the conversation already knows; a change is sent as a new
    /// `role: "system"` message rather than by editing the system prompt.
    conv_instructions: String,
    /// The instructions the running answer sent, adopted once it succeeds.
    turn_instructions: Option<String>,
    focus_input: bool,
    /// The last microphone press: whether voice typing started, and when (ms).
    dictation: Option<(Receiver<Result<(), String>>, f64)>,
}

impl Default for Assistant {
    fn default() -> Self {
        Assistant {
            items: vec![],
            input: String::new(),
            settings: Settings::default(),
            loaded: false,
            show_settings: false,
            history: vec![],
            ds_history: vec![],
            turn_provider: Provider::Claude,
            rx: None,
            stop: Arc::new(AtomicBool::new(false)),
            pending: None,
            allow_all: false,
            turn_start: None,
            conv_system: None,
            conv_instructions: String::new(),
            turn_instructions: None,
            focus_input: true,
            dictation: None,
        }
    }
}

impl Assistant {
    fn busy(&self) -> bool {
        self.rx.is_some()
    }

    fn new_chat(&mut self) {
        self.items.clear();
        self.history.clear();
        self.ds_history.clear();
        self.conv_system = None;
        self.conv_instructions.clear();
        self.allow_all = false;
    }
}

/// Portuguese or English text, following the interface language.
fn l(app: &SheetApp, pt: &'static str, en: &'static str) -> &'static str {
    if app.ui.language == crate::i18n::Language::PtBr { pt } else { en }
}

// ------------------------------------------------------------------ prompts and context

fn claude_system_prompt(locale: FormulaLocale, instructions: &str) -> String {
    let user_syntax = match locale {
        FormulaLocale::PtBr => {
            "The user's interface is Brazilian Portuguese: answer in Portuguese (Brazil), and when you show a formula to the user \
             in text, write it the way they type it: Portuguese function names (SOMA, SE, PROCV, PROCX, CONT.SE, SOMASES), \
             semicolons between arguments and a decimal comma."
        }
        FormulaLocale::Es => "Answer in Spanish; show formulas to the user in Spanish Excel syntax (SUMA, SI; semicolons).",
        _ => "Answer in the user's language.",
    };
    let mut p = format!(
        "You are the spreadsheet assistant built into GridCraft, an Excel-compatible spreadsheet app, working like an in-app spreadsheet copilot. \
You operate directly on the user's open workbook with the tools provided.

How to work:
- Look before you change: use inspect_workbook and read_range to understand the sheet, its headers and formulas.
- Tool arguments always use canonical English Excel syntax, whatever the interface language: English function names \
(SUM, IF, VLOOKUP, XLOOKUP), commas between arguments, a dot as decimal separator. GridCraft shows them to the user localized.
- Write in batches: write_range with a 2-D array for many cells at once, then format whole ranges at once.
- Keep formula relationships intact: when changing assumptions, edit the input cells, not the formulas that depend on them.
- Use execute_command for any GridCraft command (list_commands shows ids and params), e.g. home.fillColor, home.fontColor, \
home.borders, home.numberFormat, home.columnWidth, home.autofitColumnWidth, home.mergeCenter, view.freezePanes, \
home.conditionalFormat, data validation and sorting/filtering.
- When you build a sheet, make it look professional: a title, bold headers with a fill colour, sensible column widths, \
borders, number formats (Brazilian currency: format code \"R$\" #,##0.00 ; percentages: 0%), totals with formulas, \
frozen header rows, conditional formatting for problems, and a chart when it helps. Prefer create_table for tabular data.
- Do not overwrite existing data unless the user asked; build new content in an empty area or a new sheet.
- When you mention cells or ranges in your messages, cite them as [[B4]] or [[Sheet1!B4:C9]] (double square brackets); \
they become clickable links. Cite the key cells you created or used.
- Finish with a short summary of what you did. Be concise; no long preambles.

{user_syntax}"
    );
    if !instructions.trim().is_empty() {
        p.push_str("\n\nThe user's standing instructions for every conversation:\n");
        p.push_str(instructions.trim());
    }
    p
}

fn gemini_system_prompt(locale: FormulaLocale, instructions: &str) -> String {
    let syntax = match locale {
        FormulaLocale::PtBr => {
            "Responda sempre em português do Brasil. Escreva as fórmulas na sintaxe do Excel em português do Brasil: \
             nomes de funções em português (SOMA, SE, PROCV, PROCX, CONT.SE, SOMASES, ÍNDICE, CORRESP...), \
             ponto e vírgula (;) entre os argumentos, vírgula como separador decimal e VERDADEIRO/FALSO como valores lógicos."
        }
        FormulaLocale::Es => {
            "Responde siempre en español. Escribe las fórmulas con la sintaxis de Excel en español: nombres de funciones \
             en español, punto y coma (;) entre argumentos y coma decimal."
        }
        _ => "Answer in English. Write formulas in English Excel syntax with commas between arguments.",
    };
    let mut p = format!(
        "Você é o Assistente do GridCraft, uma planilha compatível com o Excel. \
         Ajude a criar fórmulas, explicar fórmulas, analisar e organizar os dados da planilha. {syntax} \
         Use os endereços reais da planilha descrita no contexto. Seja curto e prático. Sempre que sugerir uma fórmula, \
         coloque cada fórmula sozinha num bloco de código marcado como ```formula, começando com =, e diga em qual célula \
         ela deve ficar. Quando o pedido for criar ou preencher várias células, responda com um bloco ```planilha com uma \
         célula por linha no formato `A1: conteúdo`. Cite células como [[B4]]."
    );
    if !instructions.trim().is_empty() {
        p.push_str("\n\nInstruções fixas do usuário:\n");
        p.push_str(instructions.trim());
    }
    p
}

/// The sheet the question is about: active cell, selection and a preview of the used range,
/// formulas shown in `display_locale`.
fn sheet_context(app: &SheetApp, display_locale: FormulaLocale) -> String {
    let Some(d) = app.session.active() else { return String::new() };
    let Some(sh) = d.wb.active() else { return String::new() };
    let sheet_index = d.wb.active_sheet;
    let shown = |c: CellRef| -> String {
        let input = sh.input_text(c);
        let value = gridcraft_engine::display::cell_text(&d.wb, sh, c);
        if input.starts_with('=') {
            let formula = crate::formula_locale::display(&input, display_locale, &d.wb, sheet_index);
            format!("{value} [{formula}]")
        } else {
            value
        }
    };
    let active = d.selection.active;
    let sheets: Vec<String> = d.wb.sheets.iter().map(|s| s.name.clone()).collect();
    let current = {
        let s = shown(active);
        if s.is_empty() { "empty".to_string() } else { s }
    };
    let mut out = format!(
        "Workbook sheets: {}\nActive sheet: {}\nActive cell: {} ({current})\nSelection: {}\n",
        sheets.join(", "),
        sh.name,
        active.a1(),
        d.selection.ranges.iter().map(|r| r.a1()).collect::<Vec<_>>().join(", "),
    );
    let Some(used) = sh.used_range() else {
        out.push_str("The active sheet is empty.\n");
        return out;
    };
    out.push_str(&format!("Used range: {}\n", used.a1()));
    let last_row = used.end.row.min(used.start.row + 59);
    let last_col = used.end.col.min(used.start.col + 19);
    out.push_str("Preview (row | tab-separated columns; formulas in brackets):\n\t");
    for c in used.start.col..=last_col {
        out.push_str(RangeRef::cols(c, c).a1().split(':').next().unwrap_or(""));
        out.push('\t');
    }
    out.push('\n');
    for r in used.start.row..=last_row {
        out.push_str(&format!("{}\t", r + 1));
        for c in used.start.col..=last_col {
            out.push_str(&shown(CellRef::new(r, c)).replace(['\t', '\n'], " "));
            out.push('\t');
        }
        out.push('\n');
        if out.len() > MAX_CONTEXT_CHARS {
            out.push_str("(preview cut)\n");
            break;
        }
    }
    if used.end.row > last_row || used.end.col > last_col {
        out.push_str("(only the top-left part is shown; use read_range for more)\n");
    }
    out
}

// ------------------------------------------------------------------ HTTP

#[cfg(not(target_arch = "wasm32"))]
fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(600)))
        .http_status_as_error(false)
        .build()
        .into()
}

/// Busy or rate-limited: worth waiting and asking again.
fn transient(status: u16) -> bool {
    matches!(status, 408 | 429 | 500 | 502 | 503 | 504 | 529)
}

fn http_error_text(status: Option<u16>, detail: &str, pt: bool) -> String {
    let (pt_msg, en_msg) = match status {
        None => ("Não foi possível conectar. Confira sua conexão com a internet.", "Could not connect. Check your internet connection."),
        Some(400) => ("O pedido foi recusado pelo serviço.", "The service rejected the request."),
        Some(401 | 403) => (
            "A chave de API foi recusada. Abra as configurações (⚙) e confira a chave.",
            "The API key was refused. Open settings (⚙) and check it.",
        ),
        Some(402) => ("Saldo insuficiente na conta do serviço. Adicione créditos e tente de novo.", "Insufficient balance on the service account. Add credits and try again."),
        Some(404) => ("O modelo escolhido não foi encontrado.", "The chosen model was not found."),
        Some(429) => ("Limite de uso atingido. Espere um pouco e tente de novo.", "Rate limit reached. Wait a moment and try again."),
        Some(500..=599) => ("O serviço está sobrecarregado agora. Tente daqui a pouco.", "The service is overloaded right now. Try again shortly."),
        Some(_) => ("O serviço não respondeu.", "The service did not answer."),
    };
    let msg = if pt { pt_msg } else { en_msg };
    match status {
        Some(s) => format!("{msg}\n({s}: {detail})"),
        None => format!("{msg}\n({detail})"),
    }
}

// ------------------------------------------------------------------ Claude agent loop

struct ClaudeJob {
    key: String,
    model: String,
    effort: String,
    system: String,
    tools: Vec<Json>,
    history: Vec<Json>,
    pt: bool,
}

/// Claude tool definitions from the registered MCP catalog.
fn claude_tools() -> Vec<Json> {
    let Some((defs, _)) = TOOLS.get() else { return vec![] };
    defs()
        .into_iter()
        .filter(|t| t["name"].as_str().is_some_and(|n| !HIDDEN_TOOLS.contains(&n)))
        .map(|t| {
            json!({
                "name": t["name"],
                "description": t["description"],
                "input_schema": t["inputSchema"],
                "eager_input_streaming": true,
            })
        })
        .collect()
}

/// One streamed response: its content blocks (as the API sent them) and stop reason.
#[cfg(not(target_arch = "wasm32"))]
fn claude_stream(job: &ClaudeJob, tx: &Sender<Event>, stop: &AtomicBool) -> Result<(Vec<Json>, String), String> {
    let haiku = job.model.contains("haiku");
    let mut body = json!({
        "model": job.model,
        "max_tokens": 32000,
        "stream": true,
        "system": [{"type": "text", "text": job.system}],
        "tools": job.tools,
        "messages": job.history,
        "output_config": {"effort": job.effort},
        "cache_control": {"type": "ephemeral"},
    });
    // Should a replayed thinking block still not match its conversation, drop it (and the ones
    // after it) instead of failing the whole request.
    let mut thinking = json!({"type": "adaptive", "block_binding": {"prefix_mismatch_behavior": "drop_block"}});
    let mut betas = vec!["thinking-binding-controls-2026-08-01"];
    if !haiku {
        // Between-tool progress notes as short text, the reasoning itself hidden.
        thinking["display"] = json!("updates");
        betas.push("thinking-display-updates-2026-08-18");
        // A declined request is re-run on Anthropic's recommended model instead of failing.
        body["fallbacks"] = json!("default");
        betas.push("server-side-fallback-2026-07-01");
    }
    body["thinking"] = thinking;
    let mut attempt = 0u64;
    let mut resp = loop {
        let mut req = agent()
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &job.key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json");
        if !betas.is_empty() {
            req = req.header("anthropic-beta", &betas.join(","));
        }
        let mut resp = req.send(body.to_string()).map_err(|e| http_error_text(None, &e.to_string(), job.pt))?;
        let status = resp.status().as_u16();
        if status == 200 {
            break resp;
        }
        let text = resp.body_mut().read_to_string().unwrap_or_default();
        let detail =
            serde_json::from_str::<Json>(&text).ok().and_then(|v| v["error"]["message"].as_str().map(str::to_string)).unwrap_or(text);
        if transient(status) && attempt < 3 && !stop.load(Ordering::Relaxed) {
            attempt += 1;
            std::thread::sleep(std::time::Duration::from_millis(1500 * attempt));
            continue;
        }
        return Err(http_error_text(Some(status), &detail, job.pt));
    };

    parse_stream(std::io::BufReader::new(resp.body_mut().as_reader()), tx, stop, job.pt)
}

/// Reads a Messages API event stream into the response's content blocks and stop reason,
/// forwarding text as it arrives and each non-empty progress note when its block ends.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn parse_stream(reader: impl std::io::BufRead, tx: &Sender<Event>, stop: &AtomicBool, pt: bool) -> Result<(Vec<Json>, String), String> {
    let mut blocks: std::collections::BTreeMap<u64, Json> = Default::default();
    let mut partial: std::collections::HashMap<u64, String> = Default::default();
    let mut stop_reason = String::new();
    for line in reader.lines() {
        if stop.load(Ordering::Relaxed) {
            return Err("stopped".into());
        }
        let line = line.map_err(|e| http_error_text(None, &e.to_string(), pt))?;
        let Some(data) = line.strip_prefix("data:") else { continue };
        let Ok(ev) = serde_json::from_str::<Json>(data.trim()) else { continue };
        let index = ev["index"].as_u64().unwrap_or(0);
        match ev["type"].as_str().unwrap_or("") {
            "content_block_start" => {
                blocks.insert(index, ev["content_block"].clone());
            }
            "content_block_delta" => {
                let Some(block) = blocks.get_mut(&index) else { continue };
                let delta = &ev["delta"];
                match delta["type"].as_str().unwrap_or("") {
                    "text_delta" => {
                        let t = delta["text"].as_str().unwrap_or("");
                        let cur = block["text"].as_str().unwrap_or("").to_string();
                        block["text"] = json!(cur + t);
                        let _ = tx.send(Event::Text(t.to_string()));
                    }
                    "thinking_delta" => {
                        let t = delta["thinking"].as_str().unwrap_or("");
                        let cur = block["thinking"].as_str().unwrap_or("").to_string();
                        block["thinking"] = json!(cur + t);
                    }
                    "signature_delta" => block["signature"] = delta["signature"].clone(),
                    "input_json_delta" => partial.entry(index).or_default().push_str(delta["partial_json"].as_str().unwrap_or("")),
                    _ => {}
                }
            }
            "content_block_stop" => {
                let Some(block) = blocks.get_mut(&index) else { continue };
                match block["type"].as_str().unwrap_or("") {
                    "tool_use" => {
                        let raw = partial.remove(&index).unwrap_or_default();
                        if raw.trim().is_empty() {
                            block["input"] = json!({});
                        } else {
                            match serde_json::from_str::<Json>(&raw) {
                                Ok(v) if v.is_object() => block["input"] = v,
                                _ => {
                                    // Truncated or malformed arguments: answered with an error below.
                                    block["input"] = json!({});
                                    block["__invalid"] = json!(raw);
                                }
                            }
                        }
                    }
                    "thinking" => {
                        let note = block["thinking"].as_str().unwrap_or("").trim().to_string();
                        if !note.is_empty() {
                            let _ = tx.send(Event::Progress(note));
                        }
                    }
                    _ => {}
                }
            }
            "message_delta" => {
                if let Some(r) = ev["delta"]["stop_reason"].as_str() {
                    stop_reason = r.to_string();
                }
            }
            "error" => {
                let msg = ev["error"]["message"].as_str().unwrap_or("stream error").to_string();
                let status = if ev["error"]["type"].as_str() == Some("overloaded_error") { 529 } else { 500 };
                return Err(http_error_text(Some(status), &msg, pt));
            }
            _ => {}
        }
    }
    Ok((blocks.into_values().collect(), stop_reason))
}

#[cfg(target_arch = "wasm32")]
fn claude_stream(_job: &ClaudeJob, _tx: &Sender<Event>, _stop: &AtomicBool) -> Result<(Vec<Json>, String), String> {
    Err("The assistant is available in the desktop app.".into())
}

/// The manual tool loop: stream a response, run its tool calls on the UI thread, send the
/// results back, until Claude ends its turn.
fn run_claude(mut job: ClaudeJob, tx: Sender<Event>, stop: Arc<AtomicBool>) {
    let result = (|| -> Result<Option<Vec<Json>>, String> {
        for _ in 0..MAX_ROUNDS {
            let (mut blocks, stop_reason) = claude_stream(&job, &tx, &stop)?;
            let invalid: Vec<(String, usize)> = blocks
                .iter_mut()
                .filter_map(|b| {
                    let raw = b.as_object_mut()?.remove("__invalid")?;
                    Some((b["id"].as_str().unwrap_or("").to_string(), raw.as_str().map_or(0, str::len)))
                })
                .collect();
            job.history.push(json!({"role": "assistant", "content": blocks}));
            match stop_reason.as_str() {
                "tool_use" => {}
                "refusal" => {
                    return Err(if job.pt { "O Claude recusou este pedido." } else { "Claude declined this request." }.into());
                }
                "max_tokens" => {
                    let note = if job.pt { "\n\n(A resposta ficou longa demais e foi cortada.)" } else { "\n\n(The answer was cut off.)" };
                    let _ = tx.send(Event::Text(note.into()));
                    return Ok(Some(job.history));
                }
                _ => return Ok(Some(job.history)),
            }
            let calls: Vec<Json> = blocks.iter().filter(|b| b["type"] == "tool_use").cloned().collect();
            let mut results = vec![];
            for call in calls {
                let id = call["id"].as_str().unwrap_or("").to_string();
                let (text, is_error) = if let Some((_, len)) = invalid.iter().find(|(i, _)| *i == id) {
                    (format!("INVALID_JSON: the tool arguments were not valid JSON ({len} chars). Send the call again."), true)
                } else {
                    let (reply, answer) = std::sync::mpsc::channel();
                    let name = call["name"].as_str().unwrap_or("").to_string();
                    tx.send(Event::ToolCall { name, input: call["input"].clone(), reply }).map_err(|_| "stopped".to_string())?;
                    answer.recv().map_err(|_| "stopped".to_string())?
                };
                results.push(json!({"type": "tool_result", "tool_use_id": id, "content": text, "is_error": is_error}));
            }
            if stop.load(Ordering::Relaxed) {
                return Err("stopped".into());
            }
            job.history.push(json!({"role": "user", "content": results}));
        }
        let note = if job.pt { "\n\n(Parei: muitas etapas nesta resposta.)" } else { "\n\n(Stopped: too many steps.)" };
        let _ = tx.send(Event::Text(note.into()));
        Ok(Some(job.history))
    })();
    let _ = tx.send(Event::Done(result));
}

// ------------------------------------------------------------------ DeepSeek agent loop (OpenAI-style API)

struct DeepSeekJob {
    key: String,
    model: String,
    effort: String,
    tools: Vec<Json>,
    history: Vec<Json>,
    pt: bool,
}

/// DeepSeek tool definitions (OpenAI `function` tools) from the registered MCP catalog.
fn deepseek_tools() -> Vec<Json> {
    claude_tools()
        .into_iter()
        .map(|t| json!({"type": "function", "function": {"name": t["name"], "description": t["description"], "parameters": t["input_schema"]}}))
        .collect()
}

/// The message history for the next DeepSeek request: the system message opens the
/// conversation, and later instruction changes are appended as system messages.
fn deepseek_turn(a: &mut Assistant, locale: FormulaLocale, instructions: &str, user_text: String) -> Vec<Json> {
    if a.ds_history.is_empty() {
        a.ds_history.push(json!({"role": "system", "content": claude_system_prompt(locale, instructions)}));
        a.conv_instructions = instructions.to_string();
    }
    let mut history = a.ds_history.clone();
    a.turn_instructions = None;
    if instructions.trim() != a.conv_instructions.trim() {
        history.push(json!({"role": "system", "content": format!("The user changed their standing instructions. From now on follow these instead:\n{}", instructions.trim())}));
        a.turn_instructions = Some(instructions.to_string());
    }
    history.push(json!({"role": "user", "content": user_text}));
    history
}

/// Reads an OpenAI-style chat completion stream into the assistant message (content,
/// `reasoning_content` and `tool_calls`, as the API needs them back) and the finish reason.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn parse_openai_stream(reader: impl std::io::BufRead, tx: &Sender<Event>, stop: &AtomicBool, pt: bool) -> Result<(Json, String), String> {
    let mut content = String::new();
    let mut reasoning = String::new();
    // (id, name, arguments) per tool-call index.
    let mut calls: std::collections::BTreeMap<u64, (String, String, String)> = Default::default();
    let mut finish = String::new();
    for line in reader.lines() {
        if stop.load(Ordering::Relaxed) {
            return Err("stopped".into());
        }
        let line = line.map_err(|e| http_error_text(None, &e.to_string(), pt))?;
        let Some(data) = line.strip_prefix("data:") else { continue };
        let data = data.trim();
        if data == "[DONE]" {
            break;
        }
        let Ok(ev) = serde_json::from_str::<Json>(data) else { continue };
        if let Some(msg) = ev["error"]["message"].as_str() {
            return Err(http_error_text(Some(500), msg, pt));
        }
        let choice = &ev["choices"][0];
        let delta = &choice["delta"];
        if let Some(t) = delta["reasoning_content"].as_str() {
            reasoning.push_str(t);
        }
        if let Some(t) = delta["content"].as_str()
            && !t.is_empty()
        {
            content.push_str(t);
            let _ = tx.send(Event::Text(t.to_string()));
        }
        for call in delta["tool_calls"].as_array().into_iter().flatten() {
            let entry = calls.entry(call["index"].as_u64().unwrap_or(0)).or_default();
            if let Some(id) = call["id"].as_str() {
                entry.0 = id.to_string();
            }
            if let Some(name) = call["function"]["name"].as_str() {
                entry.1.push_str(name);
            }
            if let Some(args) = call["function"]["arguments"].as_str() {
                entry.2.push_str(args);
            }
        }
        if let Some(r) = choice["finish_reason"].as_str() {
            finish = r.to_string();
        }
    }
    let mut message = json!({"role": "assistant", "content": content});
    if !reasoning.is_empty() {
        message["reasoning_content"] = json!(reasoning);
    }
    if !calls.is_empty() {
        message["tool_calls"] = calls
            .into_values()
            .map(|(id, name, args)| json!({"id": id, "type": "function", "function": {"name": name, "arguments": args}}))
            .collect();
    }
    Ok((message, finish))
}

#[cfg(not(target_arch = "wasm32"))]
fn deepseek_stream(job: &DeepSeekJob, tx: &Sender<Event>, stop: &AtomicBool) -> Result<(Json, String), String> {
    let body = json!({
        "model": job.model,
        "messages": job.history,
        "tools": job.tools,
        "stream": true,
        "thinking": {"type": "enabled"},
        "reasoning_effort": if job.effort == "low" { "low" } else { "high" },
    });
    let mut attempt = 0u64;
    let mut resp = loop {
        let mut resp = agent()
            .post("https://api.deepseek.com/chat/completions")
            .header("Authorization", &format!("Bearer {}", job.key))
            .header("Content-Type", "application/json")
            .send(body.to_string())
            .map_err(|e| http_error_text(None, &e.to_string(), job.pt))?;
        let status = resp.status().as_u16();
        if status == 200 {
            break resp;
        }
        let text = resp.body_mut().read_to_string().unwrap_or_default();
        let detail =
            serde_json::from_str::<Json>(&text).ok().and_then(|v| v["error"]["message"].as_str().map(str::to_string)).unwrap_or(text);
        if transient(status) && attempt < 3 && !stop.load(Ordering::Relaxed) {
            attempt += 1;
            std::thread::sleep(std::time::Duration::from_millis(1500 * attempt));
            continue;
        }
        return Err(http_error_text(Some(status), &detail, job.pt));
    };
    parse_openai_stream(std::io::BufReader::new(resp.body_mut().as_reader()), tx, stop, job.pt)
}

#[cfg(target_arch = "wasm32")]
fn deepseek_stream(_job: &DeepSeekJob, _tx: &Sender<Event>, _stop: &AtomicBool) -> Result<(Json, String), String> {
    Err("The assistant is available in the desktop app.".into())
}

/// The same tool loop as Claude's, over DeepSeek's chat completions.
fn run_deepseek(mut job: DeepSeekJob, tx: Sender<Event>, stop: Arc<AtomicBool>) {
    let result = (|| -> Result<Option<Vec<Json>>, String> {
        for _ in 0..MAX_ROUNDS {
            let (message, finish) = deepseek_stream(&job, &tx, &stop)?;
            let calls: Vec<Json> = message["tool_calls"].as_array().cloned().unwrap_or_default();
            // The whole message goes back, reasoning included: DeepSeek requires it with tools.
            job.history.push(message);
            if calls.is_empty() {
                if finish == "length" {
                    let note = if job.pt { "\n\n(A resposta ficou longa demais e foi cortada.)" } else { "\n\n(The answer was cut off.)" };
                    let _ = tx.send(Event::Text(note.into()));
                }
                return Ok(Some(job.history));
            }
            for call in calls {
                let id = call["id"].as_str().unwrap_or("").to_string();
                let name = call["function"]["name"].as_str().unwrap_or("").to_string();
                let raw = call["function"]["arguments"].as_str().unwrap_or("");
                let input = if raw.trim().is_empty() { Some(json!({})) } else { serde_json::from_str::<Json>(raw).ok().filter(Json::is_object) };
                let (text, is_error) = match input {
                    None => (format!("INVALID_JSON: the tool arguments were not valid JSON ({} chars). Send the call again.", raw.len()), true),
                    Some(input) => {
                        let (reply, answer) = std::sync::mpsc::channel();
                        tx.send(Event::ToolCall { name, input, reply }).map_err(|_| "stopped".to_string())?;
                        answer.recv().map_err(|_| "stopped".to_string())?
                    }
                };
                let text = if is_error { format!("ERROR: {text}") } else { text };
                job.history.push(json!({"role": "tool", "tool_call_id": id, "content": text}));
            }
            if stop.load(Ordering::Relaxed) {
                return Err("stopped".into());
            }
        }
        let note = if job.pt { "\n\n(Parei: muitas etapas nesta resposta.)" } else { "\n\n(Stopped: too many steps.)" };
        let _ = tx.send(Event::Text(note.into()));
        Ok(Some(job.history))
    })();
    let _ = tx.send(Event::Done(result));
}

// ------------------------------------------------------------------ Gemini (plain chat)

#[cfg(not(target_arch = "wasm32"))]
fn gemini_request(key: &str, model: &str, body: &Json) -> Result<String, (Option<u16>, String)> {
    let url = format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent");
    let mut resp = agent()
        .post(&url)
        .header("x-goog-api-key", key)
        .header("Content-Type", "application/json")
        .send(body.to_string())
        .map_err(|e| (None, e.to_string()))?;
    let status = resp.status().as_u16();
    let text = resp.body_mut().read_to_string().map_err(|e| (None, e.to_string()))?;
    let parsed: Json = serde_json::from_str(&text).unwrap_or(Json::Null);
    if status != 200 {
        return Err((Some(status), parsed["error"]["message"].as_str().unwrap_or(&text).to_string()));
    }
    let answer: String = parsed["candidates"][0]["content"]["parts"]
        .as_array()
        .map(|parts| parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join(""))
        .unwrap_or_default();
    if answer.trim().is_empty() {
        let reason =
            parsed["candidates"][0]["finishReason"].as_str().or(parsed["promptFeedback"]["blockReason"].as_str()).unwrap_or("empty");
        return Err((Some(200), format!("no text ({reason})")));
    }
    Ok(answer)
}

#[cfg(target_arch = "wasm32")]
fn gemini_request(_key: &str, _model: &str, _body: &Json) -> Result<String, (Option<u16>, String)> {
    Err((None, "The assistant is available in the desktop app.".into()))
}

/// Retries when Google is overloaded, then tries the fallback models.
fn ask_gemini(key: &str, model: &str, body: &Json, pt: bool) -> Result<String, String> {
    let mut models = vec![model.to_string()];
    models.extend(GEMINI_FALLBACKS.iter().map(|m| m.to_string()).filter(|m| m != model));
    let mut last: (Option<u16>, String) = (None, String::new());
    for m in &models {
        for attempt in 0..3u64 {
            match gemini_request(key, m, body) {
                Ok(answer) => return Ok(answer),
                Err(e) => {
                    let retry = e.0.is_some_and(transient);
                    let next_model = retry || e.0 == Some(404);
                    last = e;
                    if !next_model {
                        return Err(http_error_text(last.0, &last.1, pt));
                    }
                    if !retry {
                        break;
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    std::thread::sleep(std::time::Duration::from_millis(1500 * (attempt + 1)));
                }
            }
        }
    }
    Err(http_error_text(last.0, &last.1, pt))
}

// ------------------------------------------------------------------ sending

#[cfg(not(target_arch = "wasm32"))]
fn spawn(f: impl FnOnce() + Send + 'static) {
    std::thread::spawn(f);
}

#[cfg(target_arch = "wasm32")]
fn spawn(f: impl FnOnce() + Send + 'static) {
    f();
}

#[cfg(target_arch = "wasm32")]
fn repainting(_ctx: egui::Context, out: Sender<Event>) -> Sender<Event> {
    out
}

/// Forwards worker events and repaints, so the pane wakes up for each streamed piece.
#[cfg(not(target_arch = "wasm32"))]
fn repainting(ctx: egui::Context, out: Sender<Event>) -> Sender<Event> {
    let (tx, rx) = std::sync::mpsc::channel::<Event>();
    spawn(move || {
        for ev in rx {
            let done = matches!(ev, Event::Done(_));
            if out.send(ev).is_err() {
                break;
            }
            ctx.request_repaint();
            if done {
                break;
            }
        }
    });
    tx
}

/// The system prompt and message history for the next Claude request. The history is
/// append-only and the system prompt is fixed for the conversation, because every thinking block
/// is bound to both; changed standing instructions go in as a mid-conversation system message.
fn claude_turn(a: &mut Assistant, locale: FormulaLocale, instructions: &str, user_text: String) -> (String, Vec<Json>) {
    if a.history.is_empty() || a.conv_system.is_none() {
        a.history.clear();
        a.conv_system = Some(claude_system_prompt(locale, instructions));
        a.conv_instructions = instructions.to_string();
    }
    let mut history = a.history.clone();
    history.push(json!({"role": "user", "content": [{"type": "text", "text": user_text}]}));
    a.turn_instructions = None;
    if instructions.trim() != a.conv_instructions.trim() {
        let update = if instructions.trim().is_empty() {
            "The user removed their standing instructions; ignore the earlier ones.".to_string()
        } else {
            format!("The user changed their standing instructions. From now on follow these instead:\n{}", instructions.trim())
        };
        history.push(json!({"role": "system", "content": update}));
        a.turn_instructions = Some(instructions.to_string());
    }
    (a.conv_system.clone().unwrap_or_default(), history)
}

fn send(app: &mut SheetApp, ctx: &egui::Context, question: String) {
    let locale = app.ui.language.formula_locale();
    let pt = locale == FormulaLocale::PtBr;
    let settings = app.assistant.settings.clone();
    // Claude reads and writes canonical formulas through its tools; Gemini answers in the user's syntax.
    let context = sheet_context(app, if settings.provider == Provider::Gemini { locale } else { FormulaLocale::En });
    let start = app.session.active().map(|d| (d.uid, d.undo.len(), d.wb.clone()));
    let (out, rx) = std::sync::mpsc::channel();
    let tx = repainting(ctx.clone(), out);
    let stop = Arc::new(AtomicBool::new(false));
    let a = &mut app.assistant;
    a.items.push(Item::User(question.clone()));
    a.rx = Some(rx);
    a.stop = stop.clone();
    a.turn_start = start;
    a.turn_provider = settings.provider;
    match settings.provider {
        Provider::Claude => {
            let user_text = format!("<sheet_context>\n{context}</sheet_context>\n\n{question}");
            let (system, history) = claude_turn(a, locale, &settings.instructions, user_text);
            let job = ClaudeJob {
                key: settings.claude_key.clone(),
                model: settings.claude_model.clone(),
                effort: settings.effort.clone(),
                system,
                tools: claude_tools(),
                history,
                pt,
            };
            spawn(move || run_claude(job, tx, stop));
        }
        Provider::DeepSeek => {
            let user_text = format!("<sheet_context>\n{context}</sheet_context>\n\n{question}");
            let history = deepseek_turn(a, locale, &settings.instructions, user_text);
            let job = DeepSeekJob {
                key: settings.deepseek_key.clone(),
                model: settings.deepseek_model.clone(),
                effort: settings.effort.clone(),
                tools: deepseek_tools(),
                history,
                pt,
            };
            spawn(move || run_deepseek(job, tx, stop));
        }
        Provider::Gemini => {
            // Earlier turns as they were; the sheet context rides with the newest question only.
            let mut contents: Vec<Json> = vec![];
            for item in &a.items[..a.items.len() - 1] {
                match item {
                    Item::User(t) => contents.push(json!({"role": "user", "parts": [{"text": t}]})),
                    Item::Text(t) => contents.push(json!({"role": "model", "parts": [{"text": t}]})),
                    _ => {}
                }
            }
            contents.push(json!({"role": "user", "parts": [{"text": format!("Contexto da planilha:\n{context}\nPedido: {question}")}]}));
            let body = json!({
                "systemInstruction": {"parts": [{"text": gemini_system_prompt(locale, &settings.instructions)}]},
                "contents": contents,
            });
            let (key, model) = (settings.gemini_key.clone(), settings.gemini_model.clone());
            spawn(move || match ask_gemini(&key, &model, &body, pt) {
                Ok(text) => {
                    let _ = tx.send(Event::Text(text));
                    let _ = tx.send(Event::Done(Ok(None)));
                }
                Err(e) => {
                    let _ = tx.send(Event::Done(Err(e)));
                }
            });
        }
    }
}

// ------------------------------------------------------------------ running tools on the UI thread

/// Runs one tool through the control channel; `(text, is_error)` for the model.
fn run_tool(app: &mut SheetApp, ctx: &egui::Context, name: &str, input: &Json) -> (String, bool) {
    let Some((_, caller)) = TOOLS.get() else { return ("Tools are not available in this build.".into(), true) };
    let mut backend = |method: &str, params: Json| -> Result<Json, String> {
        match crate::control::handle(app, ctx, method, &params) {
            crate::control::Outcome::Done(v) => {
                if v["ok"].as_bool() == Some(true) {
                    Ok(v["result"].clone())
                } else {
                    Err(v["error"].as_str().unwrap_or("error").to_string())
                }
            }
            crate::control::Outcome::Screenshot { .. } => {
                Err("Screenshots are not available here; use read_range or inspect_workbook.".into())
            }
        }
    };
    let r = caller(&mut backend, name, input);
    let text = r["content"]
        .as_array()
        .map(|c| c.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n"))
        .unwrap_or_default();
    // Keep tool results small: a huge read would flood the context.
    let text = if text.chars().count() > 30_000 { format!("{}…(truncated)", text.chars().take(30_000).collect::<String>()) } else { text };
    (text, r["isError"].as_bool().unwrap_or(false))
}

/// Splits `Sheet2!B3:C4` / `'My Sheet'!B3` into sheet index and range on `wb`.
fn resolve(wb: &Workbook, text: &str) -> Option<(usize, RangeRef)> {
    let (sheet, addr) = match text.rsplit_once('!') {
        Some((s, a)) => (Some(s.trim_matches('\'')), a),
        None => (None, text),
    };
    let index = match sheet {
        Some(name) => wb.sheets.iter().position(|s| s.name.eq_ignore_ascii_case(name))?,
        None => wb.active_sheet,
    };
    Some((index, RangeRef::parse(addr.trim())?))
}

/// Non-empty cells of sheet `index` inside `range` (capped scan).
fn filled_cells(wb: &Workbook, index: usize, range: RangeRef) -> usize {
    let Some(sh) = wb.sheet(index) else { return 0 };
    let Some(used) = sh.used_range() else { return 0 };
    let (r0, r1) = (range.start.row.max(used.start.row), range.end.row.min(used.end.row));
    let (c0, c1) = (range.start.col.max(used.start.col), range.end.col.min(used.end.col));
    if r1 < r0 || c1 < c0 {
        return 0;
    }
    let mut n = 0;
    let mut scanned = 0u64;
    for r in r0..=r1 {
        for c in c0..=c1 {
            scanned += 1;
            if scanned > 200_000 {
                return n;
            }
            if !sh.input_text(CellRef::new(r, c)).is_empty() {
                n += 1;
            }
        }
    }
    n
}

/// Why a tool call needs the user's go-ahead (overwriting data, deleting, files), if it does.
fn needs_confirmation(app: &SheetApp, name: &str, input: &Json) -> Option<String> {
    let pt = app.ui.language == crate::i18n::Language::PtBr;
    let d = app.session.active()?;
    let wb = &d.wb;
    let overwrite = |target: &str, rows: u32, cols: u32| -> Option<String> {
        let (index, r) = resolve(wb, target)?;
        let end_row = r.start.row + (rows.max(1) - 1).max(r.end.row - r.start.row);
        let end_col = r.start.col + (cols.max(1) - 1).max(r.end.col - r.start.col);
        let rect = RangeRef::new(r.start, CellRef::new(end_row, end_col));
        let n = filled_cells(wb, index, rect);
        (n > 0).then(|| {
            if pt {
                format!("O assistente quer sobrescrever {n} célula(s) com dados em {}.", rect.a1())
            } else {
                format!("The assistant wants to overwrite {n} cell(s) with data in {}.", rect.a1())
            }
        })
    };
    let dims = |values: &Json| -> (u32, u32) {
        let rows = values.as_array().map_or(1, Vec::len) as u32;
        let cols = values.as_array().and_then(|r| r.iter().map(|row| row.as_array().map_or(1, Vec::len)).max()).unwrap_or(1) as u32;
        (rows.max(1), cols.max(1))
    };
    let risky =
        |what: String| Some(if pt { format!("O assistente quer executar: {what}.") } else { format!("The assistant wants to run: {what}.") });
    match name {
        "write_range" => {
            let (rows, cols) = dims(&input["values"]);
            overwrite(input["range"].as_str()?, rows, cols)
        }
        "set_cell" => overwrite(input["cell"].as_str()?, 1, 1),
        "open_workbook" | "new_workbook" | "save_workbook" => risky(format!("{name} {input}")),
        "execute_command" => {
            let cmd = input["command"].as_str().unwrap_or("");
            let p = &input["params"];
            match cmd {
                "cell.set" => overwrite(p["cell"].as_str()?, 1, 1),
                "range.setValues" => {
                    let (rows, cols) = dims(&p["values"]);
                    overwrite(p["range"].as_str()?, rows, cols)
                }
                "range.fill" => overwrite(p["range"].as_str()?, 1, 1),
                c if c.starts_with("edit.clear")
                    || c.to_ascii_lowercase().contains("delete")
                    || c.contains("removeDuplicates")
                    || c.starts_with("file.")
                    || c == "sheet.hide" =>
                {
                    risky(format!("{c} {p}"))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// A short readable label for the activity list.
fn tool_label(pt: bool, name: &str, input: &Json) -> String {
    let s = |k: &str| input[k].as_str().unwrap_or("").to_string();
    let (pt_l, en_l): (String, String) = match name {
        "inspect_workbook" => ("Lendo a pasta de trabalho".into(), "Reading the workbook".into()),
        "read_range" => (format!("Lendo {}", s("range")), format!("Reading {}", s("range"))),
        "get_cell" => (format!("Lendo {}", s("cell")), format!("Reading {}", s("cell"))),
        "write_range" => (format!("Escrevendo em {}", s("range")), format!("Writing {}", s("range"))),
        "set_cell" => (format!("Escrevendo em {}", s("cell")), format!("Writing {}", s("cell"))),
        "format_range" => (format!("Formatando {}", s("range")), format!("Formatting {}", s("range"))),
        "create_table" => (format!("Criando tabela {}", s("range")), format!("Creating table {}", s("range"))),
        "insert_chart" => ("Criando gráfico".into(), "Inserting a chart".into()),
        "sort_range" => (format!("Classificando {}", s("range")), format!("Sorting {}", s("range"))),
        "filter" => ("Filtrando".into(), "Filtering".into()),
        "evaluate_formula" => (format!("Calculando {}", s("formula")), format!("Evaluating {}", s("formula"))),
        "list_commands" | "list_functions" => ("Consultando comandos".into(), "Looking up commands".into()),
        "execute_command" => {
            let p = &input["params"];
            let r = p["range"].as_str().or(p["cell"].as_str()).or(p["cols"].as_str()).unwrap_or("");
            let c = format!("{} {r}", s("command"));
            (c.clone(), c)
        }
        other => (other.to_string(), other.to_string()),
    };
    if pt { pt_l } else { en_l }.trim().to_string()
}

/// Makes everything one answer changed a single undo step labelled `label`.
fn collapse_undo(app: &mut SheetApp, start: &(u64, usize, Arc<Workbook>), label: &str) {
    let Some(d) = app.session.active_mut() else { return };
    if d.uid != start.0 || d.undo.len() <= start.1 {
        return;
    }
    if let Some(i) = d.undo[start.1..].iter().position(|e| Arc::ptr_eq(&e.wb, &start.2)) {
        let i = start.1 + i;
        d.undo.truncate(i + 1);
        d.undo[i].label = label.to_string();
    }
}

/// Applies worker events; tool calls run here, on the UI thread, or wait for confirmation.
fn poll(app: &mut SheetApp, ctx: &egui::Context) {
    if app.assistant.pending.is_some() {
        return;
    }
    let pt = app.ui.language == crate::i18n::Language::PtBr;
    loop {
        let Some(rx) = &app.assistant.rx else { return };
        let ev = match rx.try_recv() {
            Ok(ev) => ev,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Event::Done(Err("stopped".into())),
        };
        match ev {
            Event::Text(t) => match app.assistant.items.last_mut() {
                Some(Item::Text(cur)) => cur.push_str(&t),
                _ => app.assistant.items.push(Item::Text(t)),
            },
            Event::Progress(t) => app.assistant.items.push(Item::Progress(t)),
            Event::ToolCall { name, input, reply } => {
                if !app.assistant.allow_all
                    && let Some(question) = needs_confirmation(app, &name, &input)
                {
                    app.assistant.pending = Some(Pending { name, input, reply, question });
                    return;
                }
                execute(app, ctx, &name, &input, reply, pt);
            }
            Event::Done(result) => {
                let a = &mut app.assistant;
                a.rx = None;
                match result {
                    Ok(Some(history)) => {
                        if a.turn_provider == Provider::DeepSeek {
                            a.ds_history = history;
                        } else {
                            a.history = history;
                        }
                        if let Some(instructions) = a.turn_instructions.take() {
                            a.conv_instructions = instructions;
                        }
                    }
                    Ok(None) => {}
                    Err(e) if e == "stopped" => a.items.push(Item::Error(if pt { "Interrompido." } else { "Stopped." }.into())),
                    Err(e) => a.items.push(Item::Error(e)),
                }
                if let Some(start) = app.assistant.turn_start.take() {
                    collapse_undo(app, &start, if pt { "Assistente" } else { "Assistant" });
                }
                return;
            }
        }
    }
}

fn execute(app: &mut SheetApp, ctx: &egui::Context, name: &str, input: &Json, reply: Sender<(String, bool)>, pt: bool) {
    let label = tool_label(pt, name, input);
    app.assistant.items.push(Item::Tool { label, ok: None });
    let (text, is_error) = run_tool(app, ctx, name, input);
    if let Some(Item::Tool { ok, .. }) = app.assistant.items.last_mut() {
        *ok = Some(!is_error);
    }
    let _ = reply.send((text, is_error));
}

// ------------------------------------------------------------------ answers: code blocks, cells, citations

/// Splits an answer into prose (`None`) and fenced code blocks (`Some(language tag)`).
pub(crate) fn segments(text: &str) -> Vec<(Option<String>, String)> {
    let mut out = vec![];
    let mut rest = text;
    while let Some(start) = rest.find("```") {
        let before = &rest[..start];
        if !before.trim().is_empty() {
            out.push((None, before.trim().to_string()));
        }
        let after = &rest[start + 3..];
        // The language tag line: ```formula, ```planilha, ...
        let tag_end = after.find('\n').unwrap_or(after.len());
        let tag = after[..tag_end].trim().to_lowercase();
        let body = after.get(tag_end + 1..).unwrap_or("");
        match body.find("```") {
            Some(end) => {
                out.push((Some(tag), body[..end].trim().to_string()));
                rest = &body[end + 3..];
            }
            None => {
                out.push((Some(tag), body.trim().to_string()));
                rest = "";
            }
        }
    }
    if !rest.trim().is_empty() {
        out.push((None, rest.trim().to_string()));
    }
    out
}

/// A `planilha` block: one `A1: content` line per cell. `None` unless every line is a cell.
pub(crate) fn parse_cells(text: &str) -> Option<Vec<(String, String)>> {
    let mut cells = vec![];
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let line = line.trim_start_matches(['-', '*', ' ']).trim_matches('`');
        let addr_len = line.find(|c: char| !(c.is_ascii_alphanumeric() || c == '$')).unwrap_or(line.len());
        let cell = CellRef::parse(&line[..addr_len])?;
        let rest = line[addr_len..].trim_start();
        let content = rest.strip_prefix(':').or_else(|| rest.strip_prefix('|'))?.trim();
        cells.push((cell.a1(), content.to_string()));
    }
    (!cells.is_empty()).then_some(cells)
}

/// Text and `[[Sheet!A1:B2]]` citations, in order: `(is_citation, text)`.
pub(crate) fn citations(text: &str) -> Vec<(bool, String)> {
    let mut out: Vec<(bool, String)> = vec![];
    let mut push = |cite: bool, t: &str| match out.last_mut() {
        Some((false, prev)) if !cite => prev.push_str(t),
        _ => out.push((cite, t.to_string())),
    };
    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        let Some(len) = rest[start + 2..].find("]]") else { break };
        let inner = &rest[start + 2..start + 2 + len];
        let addr = inner.rsplit_once('!').map_or(inner, |(_, a)| a);
        if RangeRef::parse(addr.trim()).is_some() {
            if start > 0 {
                push(false, &rest[..start]);
            }
            push(true, inner.trim());
        } else {
            push(false, &rest[..start + 4 + len]);
        }
        rest = &rest[start + 4 + len..];
    }
    if !rest.is_empty() {
        push(false, rest);
    }
    out
}

fn go_to(app: &mut SheetApp, target: &str) {
    let Some(d) = app.session.active() else { return };
    let Some((index, range)) = resolve(&d.wb, target) else { return };
    if index != d.wb.active_sheet {
        let name = d.wb.sheets[index].name.clone();
        let _ = app.run("sheet.activate", json!({"sheet": name}));
    }
    let _ = app.run("selection.set", json!({"range": range.a1(), "active": range.start.a1()}));
    app.grid.ensure_visible = true;
}

/// Writes every cell through the normal entry path (localized formulas, decimal commas), then
/// puts the selection back. One undo step.
fn apply_cells(app: &mut SheetApp, cells: &[(String, String)]) {
    let start = app.session.active().map(|d| (d.uid, d.undo.len(), d.wb.clone()));
    let back = app.session.active().map(|d| d.selection.active.a1());
    let mut written = 0;
    for (addr, content) in cells {
        if app.run("selection.set", json!({"cell": addr})).is_err() {
            continue;
        }
        app.begin_edit(Some(content.clone()), false);
        if app.commit_edit(0, 0, false, false) {
            written += 1;
        }
    }
    if let Some(b) = back {
        let _ = app.run("selection.set", json!({"cell": b}));
    }
    let label = l(app, "Assistente", "Assistant");
    if let Some(start) = start {
        collapse_undo(app, &start, label);
    }
    app.toast = Some((format!("{written} {}", l(app, "células preenchidas", "cells filled")), crate::now_ms()));
}

fn insert_formula(app: &mut SheetApp, formula: &str) {
    let cell = app.session.active().map(|d| d.selection.active.a1()).unwrap_or_default();
    app.begin_edit(Some(formula.to_string()), false);
    let ok = app.commit_edit(0, 0, false, false);
    let msg = if ok {
        format!("{} {cell}", l(app, "Fórmula inserida em", "Formula inserted in"))
    } else {
        l(app, "Não foi possível inserir a fórmula.", "The formula could not be inserted.").to_string()
    };
    app.toast = Some((msg, crate::now_ms()));
}

/// Prose with clickable citations; light markdown (`**`, headings, bullets) is simplified.
fn prose(ui: &mut Ui, text: &str, go: &mut Option<String>) {
    for line in text.lines() {
        let line = line.replace("**", "");
        let trimmed = line.trim_start();
        let (line, heading) = match trimmed.strip_prefix("### ").or_else(|| trimmed.strip_prefix("## ")).or_else(|| trimmed.strip_prefix("# ")) {
            Some(h) => (h.to_string(), true),
            None => match trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
                Some(b) => (format!("• {b}"), false),
                None => (line.clone(), false),
            },
        };
        if line.trim().is_empty() {
            ui.add_space(4.0);
            continue;
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (cite, piece) in citations(&line) {
                if cite {
                    if ui.link(egui::RichText::new(&piece).strong()).clicked() {
                        *go = Some(piece);
                    }
                } else {
                    let rt = egui::RichText::new(piece);
                    ui.add(egui::Label::new(if heading { rt.strong() } else { rt }).wrap().selectable(true));
                }
            }
        });
    }
}

// ------------------------------------------------------------------ voice input

/// Starts the system's voice typing, which then writes into the focused text box: Windows voice
/// typing (Win+H), pressed a moment later so the question box has taken focus. Speech
/// recognition is Windows' own; nothing is recorded or sent by GridCraft.
fn start_dictation() -> Receiver<Result<(), String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    #[cfg(windows)]
    spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(250));
        let press = || -> Result<(), String> {
            use enigo::{Direction, Enigo, Key, Keyboard, Settings};
            let mut keys = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;
            keys.key(Key::Meta, Direction::Press).map_err(|e| e.to_string())?;
            let h = keys.key(Key::Unicode('h'), Direction::Click).map_err(|e| e.to_string());
            keys.key(Key::Meta, Direction::Release).map_err(|e| e.to_string())?;
            h
        };
        let _ = tx.send(press());
    });
    #[cfg(not(windows))]
    let _ = tx.send(Err("voice typing is started from the system here (macOS: press Fn twice)".into()));
    rx
}

/// A round button with a drawn microphone; red while voice typing was just started.
fn mic_button(ui: &mut Ui, listening: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::click());
    let p = ui.painter();
    let red = Color32::from_rgb(0xD0, 0x3C, 0x3C);
    if listening {
        p.circle_filled(rect.center(), 13.0, red);
    } else if resp.hovered() {
        p.circle_filled(rect.center(), 13.0, t.hover);
    }
    let ink = if listening { Color32::WHITE } else { t.text };
    let c = rect.center();
    // Capsule, cradle, stem and base.
    p.rect_filled(egui::Rect::from_center_size(c + egui::vec2(0.0, -3.0), egui::vec2(6.0, 11.0)), 3.0, ink);
    let stroke = egui::Stroke::new(1.6, ink);
    let cradle: Vec<egui::Pos2> =
        (0..=12).map(|i| std::f32::consts::PI * i as f32 / 12.0).map(|a| c + egui::vec2(-6.0 * a.cos(), -1.0 + 6.0 * a.sin())).collect();
    p.add(egui::Shape::line(cradle, stroke));
    p.line_segment([c + egui::vec2(0.0, 5.0), c + egui::vec2(0.0, 8.5)], stroke);
    p.line_segment([c + egui::vec2(-3.5, 8.5), c + egui::vec2(3.5, 8.5)], stroke);
    resp
}

// ------------------------------------------------------------------ the pane

fn settings_view(app: &mut SheetApp, ui: &mut Ui) {
    let pt = app.ui.language == crate::i18n::Language::PtBr;
    let t = |p: &'static str, e: &'static str| if pt { p } else { e };
    let s = &mut app.assistant.settings;
    let mut changed = false;
    ui.label(egui::RichText::new(t("Configurações do assistente", "Assistant settings")).font(theme::ui_bold(13.0)));
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        changed |= ui.radio_value(&mut s.provider, Provider::Claude, t("Claude (recomendado)", "Claude (recommended)")).changed();
        changed |= ui.radio_value(&mut s.provider, Provider::DeepSeek, "DeepSeek").changed();
        changed |= ui.radio_value(&mut s.provider, Provider::Gemini, "Gemini").changed();
    });
    ui.add_space(6.0);
    match s.provider {
        Provider::Claude => {
            ui.add(
                egui::Label::new(t(
                    "O Claude trabalha direto na planilha: lê, escreve, formata, cria tabelas e gráficos. Usa uma chave de API da Anthropic (cobrança por uso).",
                    "Claude works directly on the sheet: reads, writes, formats, creates tables and charts. Uses an Anthropic API key (pay per use).",
                ))
                .wrap(),
            );
            ui.hyperlink_to(t("Criar uma chave de API da Anthropic", "Create an Anthropic API key"), CLAUDE_KEY_URL);
            changed |= ui
                .add(egui::TextEdit::singleline(&mut s.claude_key).password(true).hint_text("sk-ant-…").desired_width(f32::INFINITY))
                .changed();
            ui.add_space(4.0);
            let model_name = CLAUDE_MODELS.iter().find(|m| m.0 == s.claude_model).map_or(s.claude_model.clone(), |m| (if pt { m.1 } else { m.2 }).to_string());
            egui::ComboBox::from_label(t("Modelo", "Model")).selected_text(model_name).show_ui(ui, |ui| {
                for (id, pt_name, en_name) in CLAUDE_MODELS {
                    changed |= ui.selectable_value(&mut s.claude_model, id.to_string(), if pt { *pt_name } else { *en_name }).changed();
                }
            });
            let effort_name = EFFORTS.iter().find(|e| e.0 == s.effort).map_or(s.effort.clone(), |e| (if pt { e.1 } else { e.2 }).to_string());
            egui::ComboBox::from_label(t("Velocidade", "Speed")).selected_text(effort_name).show_ui(ui, |ui| {
                for (id, pt_name, en_name) in EFFORTS {
                    changed |= ui.selectable_value(&mut s.effort, id.to_string(), if pt { *pt_name } else { *en_name }).changed();
                }
            });
        }
        Provider::DeepSeek => {
            ui.add(
                egui::Label::new(t(
                    "O DeepSeek também trabalha direto na planilha, com as mesmas ferramentas. Usa uma chave de API da DeepSeek (créditos pré-pagos).",
                    "DeepSeek also works directly on the sheet, with the same tools. Uses a DeepSeek API key (prepaid credits).",
                ))
                .wrap(),
            );
            ui.hyperlink_to(t("Criar uma chave de API da DeepSeek", "Create a DeepSeek API key"), DEEPSEEK_KEY_URL);
            changed |= ui
                .add(egui::TextEdit::singleline(&mut s.deepseek_key).password(true).hint_text("sk-…").desired_width(f32::INFINITY))
                .changed();
            ui.add_space(4.0);
            let model_name =
                DEEPSEEK_MODELS.iter().find(|m| m.0 == s.deepseek_model).map_or(s.deepseek_model.clone(), |m| (if pt { m.1 } else { m.2 }).to_string());
            egui::ComboBox::from_label(t("Modelo", "Model")).selected_text(model_name).show_ui(ui, |ui| {
                for (id, pt_name, en_name) in DEEPSEEK_MODELS {
                    changed |= ui.selectable_value(&mut s.deepseek_model, id.to_string(), if pt { *pt_name } else { *en_name }).changed();
                }
            });
            let effort_name = EFFORTS.iter().find(|e| e.0 == s.effort).map_or(s.effort.clone(), |e| (if pt { e.1 } else { e.2 }).to_string());
            egui::ComboBox::from_id_salt("deepseek_effort").selected_text(effort_name).show_ui(ui, |ui| {
                for (id, pt_name, en_name) in EFFORTS {
                    changed |= ui.selectable_value(&mut s.effort, id.to_string(), if pt { *pt_name } else { *en_name }).changed();
                }
            });
        }
        Provider::Gemini => {
            ui.add(
                egui::Label::new(t(
                    "O Gemini conversa e sugere fórmulas e blocos de células para você aplicar. Usa uma chave do Google AI Studio.",
                    "Gemini chats and suggests formulas and cell blocks for you to apply. Uses a Google AI Studio key.",
                ))
                .wrap(),
            );
            ui.hyperlink_to(t("Obter uma chave do Gemini", "Get a Gemini key"), GEMINI_KEY_URL);
            changed |= ui
                .add(egui::TextEdit::singleline(&mut s.gemini_key).password(true).hint_text("AIza…").desired_width(f32::INFINITY))
                .changed();
            ui.horizontal(|ui| {
                ui.label(t("Modelo", "Model"));
                changed |= ui.text_edit_singleline(&mut s.gemini_model).changed();
            });
        }
    }
    ui.add_space(8.0);
    ui.label(egui::RichText::new(t("Instruções fixas", "Standing instructions")).strong());
    ui.add(
        egui::Label::new(
            egui::RichText::new(t(
                "Valem para todas as conversas. Ex.: \"use R$ com 2 casas\", \"cabeçalhos em negrito e azul\".",
                "Apply to every conversation. E.g. \"currency with 2 decimals\", \"bold blue headers\".",
            ))
            .small(),
        )
        .wrap(),
    );
    changed |= ui.add(egui::TextEdit::multiline(&mut s.instructions).desired_rows(3).desired_width(f32::INFINITY)).changed();
    if changed {
        s.save();
    }
    ui.add_space(8.0);
    let ready = !s.key().is_empty();
    if ui.add_enabled(ready, egui::Button::new(t("Pronto", "Done"))).clicked() {
        app.assistant.show_settings = false;
        app.assistant.focus_input = true;
    }
}

pub fn show(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let ctx = ui.ctx().clone();
    if !app.assistant.loaded {
        app.assistant.loaded = true;
        app.assistant.settings = Settings::load();
    }
    poll(app, &ctx);
    if app.assistant.busy() {
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }

    // Model name, new chat, settings.
    let mut toggle_settings = false;
    let mut new_chat = false;
    ui.horizontal(|ui| {
        let model = app.assistant.settings.model().to_string();
        ui.label(egui::RichText::new(model).small().color(t.text_dim));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            toggle_settings = ui.small_button("⚙").on_hover_text(l(app, "Configurações", "Settings")).clicked();
            let can_reset = !app.assistant.items.is_empty() && !app.assistant.busy();
            new_chat = ui.add_enabled(can_reset, egui::Button::new(l(app, "Nova conversa", "New chat")).small()).clicked();
        });
    });
    if toggle_settings {
        app.assistant.show_settings = !app.assistant.show_settings;
    }
    if new_chat {
        app.assistant.new_chat();
    }
    if app.assistant.show_settings || app.assistant.settings.key().is_empty() {
        egui::ScrollArea::vertical().show(ui, |ui| settings_view(app, ui));
        return;
    }

    let mut ask: Option<String> = None;
    let mut insert: Option<String> = None;
    let mut apply: Option<Vec<(String, String)>> = None;
    let mut go: Option<String> = None;
    let mut decision: Option<u8> = None;
    egui::Panel::bottom("assistant_composer").frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(0, 6))).show(ui, |ui| {
        if let Some(p) = &app.assistant.pending {
            egui::Frame::NONE
                .fill(Color32::from_rgb(0xFF, 0xF4, 0xCE))
                .stroke(egui::Stroke::new(1.0, Color32::from_rgb(0xC2, 0x8B, 0x00)))
                .corner_radius(6.0)
                .inner_margin(8.0)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(egui::Label::new(egui::RichText::new(&p.question).color(Color32::from_rgb(0x4A, 0x3A, 0x00))).wrap());
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(l(app, "Permitir", "Allow")).clicked() {
                            decision = Some(1);
                        }
                        if ui.button(l(app, "Permitir sempre nesta conversa", "Always allow in this chat")).clicked() {
                            decision = Some(2);
                        }
                        if ui.button(l(app, "Recusar", "Deny")).clicked() {
                            decision = Some(0);
                        }
                    });
                });
            ui.add_space(4.0);
        }
        if app.assistant.items.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for (pt_s, en_s) in [
                    ("Explique esta planilha", "Explain this sheet"),
                    ("Crie uma fórmula para a célula ativa", "Create a formula for the active cell"),
                    ("Encontre e corrija erros", "Find and fix errors"),
                    ("Formate como uma tabela profissional", "Format this as a professional table"),
                ] {
                    let s = l(app, pt_s, en_s);
                    if ui.small_button(s).clicked() && !app.assistant.busy() {
                        ask = Some(s.to_string());
                    }
                }
            });
            ui.add_space(4.0);
        }
        let hint = l(app, "Peça algo… (Enter envia, Shift+Enter quebra a linha)", "Ask something… (Enter sends, Shift+Enter breaks the line)");
        let edit = ui.add(
            egui::TextEdit::multiline(&mut app.assistant.input)
                .hint_text(hint)
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        if app.assistant.focus_input {
            edit.request_focus();
            app.assistant.focus_input = false;
        }
        let enter = edit.has_focus() && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        ui.horizontal(|ui| {
            if app.assistant.busy() {
                if ui.button(l(app, "■ Parar", "■ Stop")).clicked() {
                    app.assistant.stop.store(true, Ordering::Relaxed);
                    if app.assistant.pending.is_some() {
                        decision = Some(0);
                    }
                }
            } else {
                let can_send = !app.assistant.input.trim().is_empty();
                if (ui.add_enabled(can_send, egui::Button::new(l(app, "Enviar", "Send"))).clicked() || (enter && can_send)) && ask.is_none() {
                    ask = Some(app.assistant.input.trim().to_string());
                    app.assistant.input.clear();
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let listening = app.assistant.dictation.as_ref().is_some_and(|(_, at)| crate::now_ms() - at < 4000.0);
                let tip = l(
                    app,
                    "Falar em vez de digitar: liga a digitação por voz do Windows (Win+H). Fale e o texto aparece na caixa; clique de novo ou faça uma pausa para parar.",
                    "Speak instead of typing: starts Windows voice typing (Win+H). Speak and the text appears in the box; click again or pause to stop.",
                );
                if mic_button(ui, listening).on_hover_text(tip).clicked() {
                    // Focus the question box first: voice typing writes into the focused field.
                    app.assistant.focus_input = true;
                    app.assistant.dictation = Some((start_dictation(), crate::now_ms()));
                }
            });
        });
        if let Some((rx, _)) = &app.assistant.dictation
            && let Ok(Err(e)) = rx.try_recv()
        {
            let msg = format!("{}\n({e})", l(app, "Não foi possível ligar a digitação por voz. Use Win+H com a caixa de pergunta selecionada.", "Could not start voice typing. Press Win+H with the question box selected."));
            app.assistant.items.push(Item::Error(msg));
            app.assistant.dictation = None;
        }
    });

    egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
        if app.assistant.items.is_empty() {
            ui.add_space(8.0);
            ui.label(egui::RichText::new(l(app, "Como posso ajudar com esta planilha?", "How can I help with this sheet?")).font(theme::ui_bold(14.0)));
            ui.add(
                egui::Label::new(
                    egui::RichText::new(l(
                        app,
                        "Posso explicar fórmulas, encontrar erros, montar modelos e planilhas formatadas, criar tabelas e gráficos.",
                        "I can explain formulas, find errors, build formatted models and sheets, create tables and charts.",
                    ))
                    .color(t.text_dim),
                )
                .wrap(),
            );
        }
        let active = app.session.active().map(|d| d.selection.active.a1()).unwrap_or_default();
        let claude = app.assistant.settings.provider != Provider::Gemini;
        let (insert_label, copy_label, apply_label, cells_label) =
            (l(app, "Inserir em", "Insert in"), l(app, "Copiar", "Copy"), l(app, "Aplicar na planilha", "Apply to sheet"), l(app, "células", "cells"));
        for item in &app.assistant.items {
            match item {
                Item::User(text) => {
                    ui.add_space(8.0);
                    egui::Frame::NONE.fill(t.hover).corner_radius(8.0).inner_margin(8.0).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(egui::Label::new(text).wrap().selectable(true));
                    });
                }
                Item::Progress(text) => {
                    ui.add(egui::Label::new(egui::RichText::new(text).italics().color(t.text_dim)).wrap());
                }
                Item::Tool { label, ok } => {
                    let (mark, color) = match ok {
                        None => ("…", t.text_dim),
                        Some(true) => ("✓", Color32::from_rgb(0x10, 0x7C, 0x41)),
                        Some(false) => ("✗", Color32::from_rgb(0xD0, 0x3C, 0x3C)),
                    };
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(mark).color(color));
                        ui.add(egui::Label::new(egui::RichText::new(label).small().color(t.text_dim)).truncate());
                    });
                }
                Item::Error(text) => {
                    ui.add_space(6.0);
                    egui::Frame::NONE
                        .fill(Color32::from_rgb(0xFD, 0xEC, 0xEA))
                        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(0xD0, 0x3C, 0x3C)))
                        .corner_radius(8.0)
                        .inner_margin(8.0)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.add(egui::Label::new(egui::RichText::new(text).color(Color32::from_rgb(0x8A, 0x1C, 0x1C))).wrap().selectable(true));
                        });
                }
                Item::Text(text) => {
                    ui.add_space(6.0);
                    for (n, (tag, part)) in segments(text).into_iter().enumerate() {
                        if tag.is_none() {
                            prose(ui, &part, &mut go);
                            continue;
                        }
                        let cells = if claude { None } else { parse_cells(&part) };
                        egui::Frame::NONE.fill(t.hover).corner_radius(4.0).inner_margin(6.0).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            egui::ScrollArea::vertical().id_salt(("code", text.len(), n)).max_height(180.0).show(ui, |ui| {
                                ui.add(egui::Label::new(egui::RichText::new(&part).monospace()).wrap().selectable(true));
                            });
                        });
                        if let Some(cells) = cells {
                            if ui.button(format!("{apply_label} ({} {cells_label})", cells.len())).clicked() {
                                apply = Some(cells);
                            }
                        } else if let Some(f) = part.lines().find(|x| x.trim_start().starts_with('=')).map(|x| x.trim().to_string()) {
                            ui.horizontal(|ui| {
                                if ui.button(format!("{insert_label} {active}")).clicked() {
                                    insert = Some(f.clone());
                                }
                                if ui.small_button(copy_label).clicked() {
                                    ui.ctx().copy_text(f.clone());
                                }
                            });
                        }
                    }
                }
            }
        }
        if app.assistant.busy() && app.assistant.pending.is_none() {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(egui::RichText::new(l(app, "Trabalhando…", "Working…")).color(t.text_dim));
            });
        }
        ui.add_space(4.0);
    });

    if let Some(choice) = decision
        && let Some(p) = app.assistant.pending.take()
    {
        let pt = app.ui.language == crate::i18n::Language::PtBr;
        if choice == 0 {
            app.assistant.items.push(Item::Tool { label: tool_label(pt, &p.name, &p.input), ok: Some(false) });
            let _ = p.reply.send(("The user declined this change. Do not retry it; ask or continue differently.".into(), true));
        } else {
            if choice == 2 {
                app.assistant.allow_all = true;
            }
            execute(app, &ctx, &p.name, &p.input, p.reply, pt);
        }
    }
    if let Some(target) = go {
        go_to(app, &target);
    }
    if let Some(f) = insert {
        insert_formula(app, &f);
    }
    if let Some(cells) = apply {
        apply_cells(app, &cells);
    }
    if let Some(q) = ask {
        send(app, &ctx, q);
        app.assistant.focus_input = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> SheetApp {
        let mut s = gridcraft_engine::Session::new();
        s.new_workbook();
        SheetApp::new(s, Default::default())
    }

    #[test]
    fn answers_split_into_prose_and_formulas() {
        let s = segments("Use isto:\n```formula\n=PROCX(D2;A:A;B:B)\n```\nPronto.");
        assert_eq!(s, [(None, "Use isto:".into()), (Some("formula".into()), "=PROCX(D2;A:A;B:B)".into()), (None, "Pronto.".into())]);
        assert_eq!(segments("só texto"), [(None, "só texto".to_string())]);
    }

    #[test]
    fn cell_blocks_parse_addresses_and_contents() {
        let cells = parse_cells("A1: Item\nB1: Valor\n\nB3: =SOMA(B2:B2)\nC2: 1,5").unwrap();
        assert_eq!(cells[2], ("B3".to_string(), "=SOMA(B2:B2)".to_string()));
        assert_eq!(cells[3].1, "1,5");
        assert!(parse_cells("=PROCX(D2;A:A;B:B)").is_none());
        assert!(parse_cells("texto solto").is_none());
    }

    #[test]
    fn citations_become_links_and_other_brackets_stay_text() {
        let c = citations("Total em [[B9]] vem de [[Plan1!B2:B8]] e [[não é célula]].");
        assert_eq!(
            c,
            [
                (false, "Total em ".to_string()),
                (true, "B9".to_string()),
                (false, " vem de ".to_string()),
                (true, "Plan1!B2:B8".to_string()),
                (false, " e [[não é célula]].".to_string()),
            ]
        );
    }

    #[test]
    fn prompts_ask_for_canonical_tool_syntax_and_portuguese_answers() {
        let p = claude_system_prompt(FormulaLocale::PtBr, "use R$");
        assert!(p.contains("canonical English") && p.contains("Portuguese (Brazil)") && p.contains("[[B4]]") && p.ends_with("use R$"));
        assert!(gemini_system_prompt(FormulaLocale::PtBr, "").contains("ponto e vírgula"));
    }

    #[test]
    fn overwrite_and_destructive_calls_need_confirmation() {
        let mut app = app();
        app.run("cell.set", json!({"cell": "B2", "input": "keep"})).unwrap();
        assert!(needs_confirmation(&app, "write_range", &json!({"range": "A1", "values": [["a", "b"], ["c", "d"]]})).is_some());
        assert!(needs_confirmation(&app, "write_range", &json!({"range": "D1", "values": [["a", "b"]]})).is_none());
        assert!(needs_confirmation(&app, "set_cell", &json!({"cell": "B2", "input": "x"})).is_some());
        assert!(needs_confirmation(&app, "execute_command", &json!({"command": "edit.clearAll", "params": {"range": "A1:C3"}})).is_some());
        assert!(needs_confirmation(&app, "execute_command", &json!({"command": "home.bold", "params": {"range": "A1:C3"}})).is_none());
    }

    #[test]
    fn stream_rebuilds_text_thinking_and_tool_calls() {
        let sse = [
            r#"{"type":"message_start","message":{"id":"msg_1"}}"#,
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Vou ler a planilha."}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig123"}}"#,
            r#"{"type":"content_block_stop","index":0}"#,
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Olá "}}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"[[B4]]"}}"#,
            r#"{"type":"content_block_stop","index":1}"#,
            r#"{"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"toolu_1","name":"read_range","input":{}}}"#,
            r#"{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"range\":"}}"#,
            r#"{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"\"A1:C9\"}"}}"#,
            r#"{"type":"content_block_stop","index":2}"#,
            r#"{"type":"content_block_start","index":3,"content_block":{"type":"tool_use","id":"toolu_2","name":"set_cell","input":{}}}"#,
            r#"{"type":"content_block_delta","index":3,"delta":{"type":"input_json_delta","partial_json":"{\"cell\":\"B"}}"#,
            r#"{"type":"content_block_stop","index":3}"#,
            r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":9}}"#,
            r#"{"type":"message_stop"}"#,
        ]
        .iter()
        .map(|d| format!("event: x\ndata: {d}\n\n"))
        .collect::<String>();
        let (tx, rx) = std::sync::mpsc::channel();
        let (blocks, stop) = parse_stream(std::io::Cursor::new(sse), &tx, &AtomicBool::new(false), true).unwrap();
        assert_eq!(stop, "tool_use");
        assert_eq!(blocks.len(), 4);
        // Thinking comes back whole, signature included, to be passed back unchanged.
        assert_eq!(blocks[0], json!({"type": "thinking", "thinking": "Vou ler a planilha.", "signature": "sig123"}));
        assert_eq!(blocks[1]["text"], "Olá [[B4]]");
        assert_eq!(blocks[2]["input"], json!({"range": "A1:C9"}));
        // Cut-off arguments are flagged, not guessed.
        assert_eq!(blocks[3]["input"], json!({}));
        assert!(blocks[3].get("__invalid").is_some());
        let events: Vec<Event> = rx.try_iter().collect();
        assert!(matches!(&events[0], Event::Progress(t) if t == "Vou ler a planilha."));
        assert!(matches!(&events[1], Event::Text(t) if t == "Olá "));
    }

    #[test]
    fn tools_run_through_the_control_channel_and_confirm_overwrites() {
        fn definitions() -> Vec<Json> {
            vec![json!({"name": "set_cell", "description": "Set a cell", "inputSchema": {"type": "object"}}), json!({"name": "screenshot"})]
        }
        fn caller(call: &mut dyn FnMut(&str, Json) -> Result<Json, String>, name: &str, args: &Json) -> Json {
            let command = if name == "set_cell" { "cell.set" } else { name };
            match call("engine.execute", json!({"command": command, "params": args})) {
                Ok(v) => json!({"content": [{"type": "text", "text": v.to_string()}], "isError": false}),
                Err(e) => json!({"content": [{"type": "text", "text": e}], "isError": true}),
            }
        }
        register_tools(definitions, caller);
        let tools = claude_tools();
        assert_eq!(tools.len(), 1, "screenshot is hidden from the pane");
        assert_eq!(tools[0]["input_schema"], json!({"type": "object"}));
        assert_eq!(tools[0]["eager_input_streaming"], json!(true));

        let mut app = app();
        let ctx = egui::Context::default();
        let (_, is_error) = run_tool(&mut app, &ctx, "set_cell", &json!({"cell": "C3", "input": "=1+1"}));
        assert!(!is_error);
        let d = app.session.active().unwrap();
        let sh = d.wb.active().unwrap();
        assert_eq!(gridcraft_engine::display::cell_text(&d.wb, sh, CellRef::parse("C3").unwrap()), "2");

        // A worker asking to overwrite C3 waits for the user; allowing runs it.
        let (tx, rx) = std::sync::mpsc::channel();
        let (reply, answer) = std::sync::mpsc::channel();
        tx.send(Event::ToolCall { name: "set_cell".into(), input: json!({"cell": "C3", "input": "9"}), reply }).unwrap();
        app.assistant.rx = Some(rx);
        poll(&mut app, &ctx);
        assert!(app.assistant.pending.is_some());
        assert!(answer.try_recv().is_err());
        let p = app.assistant.pending.take().unwrap();
        execute(&mut app, &ctx, &p.name, &p.input, p.reply, true);
        assert!(!answer.recv().unwrap().1);
        assert!(matches!(app.assistant.items.last(), Some(Item::Tool { ok: Some(true), .. })));
    }

    #[test]
    fn conversation_keeps_its_system_prompt_and_appends_instruction_changes() {
        let mut a = Assistant::default();
        let (system1, h1) = claude_turn(&mut a, FormulaLocale::PtBr, "use R$", "primeira".into());
        assert_eq!(h1.len(), 1);
        // The answer comes back; the history grows append-only.
        a.history = vec![h1[0].clone(), json!({"role": "assistant", "content": [{"type": "thinking", "thinking": "", "signature": "s"}, {"type": "text", "text": "ok"}]})];
        // Same instructions: same system prompt, nothing extra.
        let (system2, h2) = claude_turn(&mut a, FormulaLocale::PtBr, "use R$", "segunda".into());
        assert_eq!(system1, system2);
        assert_eq!(h2.len(), 3);
        assert_eq!(&h2[..2], &a.history[..]);
        // Edited instructions: the system prompt stays, the change is appended after the question.
        let (system3, h3) = claude_turn(&mut a, FormulaLocale::PtBr, "cabeçalhos azuis", "terceira".into());
        assert_eq!(system1, system3);
        assert_eq!(&h3[..2], &a.history[..]);
        assert_eq!(h3[2]["role"], "user");
        assert_eq!(h3[3]["role"], "system");
        assert!(h3[3]["content"].as_str().unwrap().ends_with("cabeçalhos azuis"));
        assert_eq!(a.turn_instructions.as_deref(), Some("cabeçalhos azuis"));
        // A new chat starts over with the current instructions in the system prompt.
        a.new_chat();
        let (system4, h4) = claude_turn(&mut a, FormulaLocale::PtBr, "cabeçalhos azuis", "nova".into());
        assert!(system4.ends_with("cabeçalhos azuis"));
        assert_eq!(h4.len(), 1);
    }

    #[test]
    fn deepseek_stream_keeps_reasoning_and_tool_calls_for_the_next_request() {
        let sse = [
            r#"{"choices":[{"index":0,"delta":{"role":"assistant","reasoning_content":"Preciso ler "}}]}"#,
            r#"{"choices":[{"index":0,"delta":{"reasoning_content":"a planilha."}}]}"#,
            r#"{"choices":[{"index":0,"delta":{"content":"Vou ver "}}]}"#,
            r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"read_range","arguments":""}}]}}]}"#,
            r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"range\":"}}]}}]}"#,
            r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"A1:B9\"}"}}]}}]}"#,
            r#"{"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
            "[DONE]",
        ]
        .iter()
        .map(|d| format!("data: {d}\n\n"))
        .collect::<String>();
        let (tx, rx) = std::sync::mpsc::channel();
        let (message, finish) = parse_openai_stream(std::io::Cursor::new(sse), &tx, &AtomicBool::new(false), true).unwrap();
        assert_eq!(finish, "tool_calls");
        assert_eq!(message["reasoning_content"], "Preciso ler a planilha.");
        assert_eq!(message["content"], "Vou ver ");
        assert_eq!(message["tool_calls"][0]["id"], "call_1");
        assert_eq!(message["tool_calls"][0]["function"]["name"], "read_range");
        assert_eq!(message["tool_calls"][0]["function"]["arguments"], r#"{"range":"A1:B9"}"#);
        assert!(matches!(rx.try_iter().next(), Some(Event::Text(t)) if t == "Vou ver "));

        // The conversation opens with the system prompt and keeps every earlier message.
        let mut a = Assistant::default();
        let h1 = deepseek_turn(&mut a, FormulaLocale::PtBr, "", "oi".into());
        assert_eq!((h1[0]["role"].as_str(), h1[1]["role"].as_str()), (Some("system"), Some("user")));
        a.ds_history = vec![h1[0].clone(), h1[1].clone(), message];
        let h2 = deepseek_turn(&mut a, FormulaLocale::PtBr, "", "de novo".into());
        assert_eq!(&h2[..3], &a.ds_history[..]);
        assert_eq!(h2[3]["content"], "de novo");
    }

    #[test]
    fn one_answer_is_one_undo_step() {
        let mut app = app();
        app.run("cell.set", json!({"cell": "A1", "input": "before"})).unwrap();
        let start = app.session.active().map(|d| (d.uid, d.undo.len(), d.wb.clone())).unwrap();
        for cell in ["B1", "B2", "B3"] {
            app.run("cell.set", json!({"cell": cell, "input": "x"})).unwrap();
        }
        collapse_undo(&mut app, &start, "Assistente");
        let d = app.session.active().unwrap();
        assert_eq!(d.undo.len(), start.1 + 1);
        assert_eq!(d.undo.last().unwrap().label, "Assistente");
        app.run("edit.undo", json!({})).unwrap();
        let sh = app.session.active().unwrap().wb.active().unwrap();
        assert_eq!(sh.input_text(CellRef::parse("A1").unwrap()), "before");
        assert!(sh.input_text(CellRef::parse("B1").unwrap()).is_empty());
    }
}
