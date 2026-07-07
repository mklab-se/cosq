//! `cosq shell` — a persistent REPL with context (profile, database,
//! container, output format), history, and completion.
//!
//! Input forms:
//! - Cosmos SQL (multi-line; complete when quotes/parens balance, or on `;`)
//! - `? <question>` — ask-mode (wired by the AI layer)
//! - `:` meta-commands — see `:help`

use anyhow::Result;
use colored::Colorize;
use cosq_client::cosmos::{CosmosClient, QueryOptions};
use cosq_core::config::{Config, Profile};
use reedline::{
    ColumnarMenu, Completer, DefaultHinter, Emacs, FileBackedHistory, KeyCode, KeyModifiers,
    MenuBuilder, Prompt, PromptEditMode, PromptHistorySearch, Reedline, ReedlineEvent,
    ReedlineMenu, Signal, Span, Suggestion, ValidationResult, Validator, default_emacs_keybindings,
};

use crate::output::{OutputFormat, write_results};

pub struct ShellContext {
    pub config: Config,
    pub profile_name: String,
    pub profile: Profile,
    pub client: CosmosClient,
    pub format: OutputFormat,
    /// Lazily cached listings for completion.
    pub known_databases: Vec<String>,
    pub known_containers: Vec<String>,
}

/// What a line of input means.
#[derive(Debug, PartialEq, Eq)]
pub enum Input {
    Sql(String),
    Ask(String),
    Meta(String, Vec<String>),
    Empty,
}

/// Classify one complete input buffer.
pub fn classify(input: &str) -> Input {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Input::Empty;
    }
    if let Some(question) = trimmed.strip_prefix('?') {
        return Input::Ask(question.trim().to_string());
    }
    if let Some(meta) = trimmed.strip_prefix(':') {
        let mut parts = meta.split_whitespace();
        let command = parts.next().unwrap_or_default().to_lowercase();
        let args: Vec<String> = parts.map(str::to_string).collect();
        return Input::Meta(command, args);
    }
    Input::Sql(trimmed.trim_end_matches(';').trim().to_string())
}

/// Is the SQL buffer complete? (`;`-terminated, or balanced on one line;
/// meta/ask lines are always complete.)
pub fn is_complete(buffer: &str) -> bool {
    let trimmed = buffer.trim();
    if trimmed.is_empty() || trimmed.starts_with(':') || trimmed.starts_with('?') {
        return true;
    }
    if trimmed.ends_with(';') {
        return true;
    }
    // single line with balanced quotes/parens is complete
    if !trimmed.contains('\n') {
        return balanced(trimmed);
    }
    false
}

fn balanced(s: &str) -> bool {
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    for ch in s.chars() {
        match quote {
            Some(q) => {
                if ch == q {
                    quote = None;
                }
            }
            None => match ch {
                '\'' | '"' => quote = Some(ch),
                '(' | '[' => depth += 1,
                ')' | ']' => depth -= 1,
                _ => {}
            },
        }
    }
    depth == 0 && quote.is_none()
}

struct SqlValidator;
impl Validator for SqlValidator {
    fn validate(&self, line: &str) -> ValidationResult {
        if is_complete(line) {
            ValidationResult::Complete
        } else {
            ValidationResult::Incomplete
        }
    }
}

struct ShellPrompt {
    left: String,
}

impl Prompt for ShellPrompt {
    fn render_prompt_left(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(&self.left)
    }
    fn render_prompt_right(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed("")
    }
    fn render_prompt_indicator(&self, _edit_mode: PromptEditMode) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed("» ")
    }
    fn render_prompt_multiline_indicator(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed("… ")
    }
    fn render_prompt_history_search_indicator(
        &self,
        _history_search: PromptHistorySearch,
    ) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed("(search) ")
    }
}

const META_COMMANDS: &[(&str, &str)] = &[
    (":help", "show this help"),
    (":quit", "leave the shell (also :exit, Ctrl-D)"),
    (":profile", ":profile <name> — switch account profile"),
    (":db", ":db <name> — switch database"),
    (":container", ":container <name> — switch container"),
    (
        ":format",
        ":format json|json-compact|table|csv — output format",
    ),
    (":queries", "list stored queries"),
    (":run", ":run <name> [--param value…] — run a stored query"),
    (":schema", "show the container's schema card"),
    (
        ":search",
        ":search <text> — semantic search in the container",
    ),
    (":explain", "explain the previous query's cost and indexing"),
];

pub struct ShellCompleter {
    pub metas: Vec<String>,
    pub databases: Vec<String>,
    pub containers: Vec<String>,
    pub stored_queries: Vec<String>,
    pub formats: Vec<String>,
}

impl ShellCompleter {
    pub fn candidates(&self, line: &str) -> Vec<String> {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix(':') {
            let mut parts = rest.split_whitespace();
            let cmd = parts.next().unwrap_or_default();
            let has_arg_started = rest.contains(' ');
            if !has_arg_started {
                return self
                    .metas
                    .iter()
                    .filter(|m| m.starts_with(&format!(":{cmd}")))
                    .cloned()
                    .collect();
            }
            let arg = parts.next_back().unwrap_or_default();
            let pool: &[String] = match cmd {
                "db" => &self.databases,
                "container" => &self.containers,
                "run" => &self.stored_queries,
                "format" => &self.formats,
                _ => return Vec::new(),
            };
            return pool
                .iter()
                .filter(|c| c.starts_with(arg))
                .cloned()
                .collect();
        }
        Vec::new()
    }
}

impl Completer for ShellCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        let prefix = &line[..pos];
        let word_start = prefix
            .rfind(char::is_whitespace)
            .map(|i| i + 1)
            .unwrap_or(0);
        self.candidates(prefix)
            .into_iter()
            .map(|value| Suggestion {
                value,
                description: None,
                style: None,
                extra: None,
                span: Span::new(word_start, pos),
                append_whitespace: true,
            })
            .collect()
    }
}

pub async fn run() -> Result<()> {
    let config = Config::load()?;
    let (profile_name, profile) = config.active(None)?;
    let profile_name = profile_name.to_string();
    let profile = profile.clone();
    let client = CosmosClient::new(&profile.account.endpoint).await?;

    let mut ctx = ShellContext {
        config,
        profile_name,
        profile,
        client,
        format: OutputFormat::Table,
        known_databases: Vec::new(),
        known_containers: Vec::new(),
    };

    // Pre-warm completion listings (best effort).
    ctx.known_databases = ctx.client.list_databases().await.unwrap_or_default();
    if let Some(db) = &ctx.profile.database {
        ctx.known_containers = ctx.client.list_containers(db).await.unwrap_or_default();
    }

    // Non-TTY stdin (pipes, scripts, tests): plain line loop, same dispatch.
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        use std::io::BufRead;
        let stdin = std::io::stdin();
        let mut buffer = String::new();
        for line in stdin.lock().lines() {
            let line = line?;
            buffer.push_str(&line);
            if !is_complete(&buffer) {
                buffer.push('\n');
                continue;
            }
            let input = std::mem::take(&mut buffer);
            match classify(&input) {
                Input::Empty => {}
                Input::Meta(cmd, args) => {
                    if handle_meta(&mut ctx, &cmd, &args).await? {
                        return Ok(());
                    }
                }
                Input::Ask(question) => handle_ask(&mut ctx, &question).await,
                Input::Sql(sql) => {
                    if let Err(e) = execute_sql(&mut ctx, &sql).await {
                        eprintln!("{} {e:#}", "error:".red().bold());
                    }
                }
            }
        }
        return Ok(());
    }

    eprintln!(
        "{} cosq shell — {} for commands, Ctrl-D to leave",
        "»".bold(),
        ":help".bold()
    );

    let mut editor = build_editor(&ctx)?;
    loop {
        let prompt = ShellPrompt {
            left: format!(
                "cosq ({}) {}{}{} ",
                ctx.profile_name,
                ctx.profile.database.as_deref().unwrap_or("-"),
                "/".dimmed(),
                ctx.profile.container.as_deref().unwrap_or("-"),
            ),
        };
        match editor.read_line(&prompt) {
            Ok(Signal::Success(line)) => match classify(&line) {
                Input::Empty => {}
                Input::Meta(cmd, args) => {
                    let quit = handle_meta(&mut ctx, &cmd, &args).await?;
                    if quit {
                        break;
                    }
                    editor = build_editor(&ctx)?; // refresh completer pools
                }
                Input::Ask(question) => {
                    handle_ask(&mut ctx, &question).await;
                }
                Input::Sql(sql) => {
                    if let Err(e) = execute_sql(&mut ctx, &sql).await {
                        eprintln!("{} {e:#}", "error:".red().bold());
                    }
                }
            },
            Ok(Signal::CtrlD) => break,
            Ok(Signal::CtrlC) => continue,
            Err(e) => {
                eprintln!("{} {e}", "error:".red().bold());
                break;
            }
        }
    }
    eprintln!("bye");
    Ok(())
}

fn build_editor(ctx: &ShellContext) -> Result<Reedline> {
    let completer = Box::new(ShellCompleter {
        metas: META_COMMANDS.iter().map(|(m, _)| m.to_string()).collect(),
        databases: ctx.known_databases.clone(),
        containers: ctx.known_containers.clone(),
        stored_queries: cosq_core::stored_query::list_query_names()
            .into_iter()
            .map(|(name, _)| name)
            .collect(),
        formats: ["json", "json-compact", "table", "csv"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
    });
    let menu = Box::new(ColumnarMenu::default().with_name("completions"));
    let mut keybindings = default_emacs_keybindings();
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu("completions".to_string()),
            ReedlineEvent::MenuNext,
        ]),
    );
    let history_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".cosq")
        .join("history");
    let history = FileBackedHistory::with_file(500, history_path)?;

    Ok(Reedline::create()
        .with_completer(completer)
        .with_menu(ReedlineMenu::EngineCompleter(menu))
        .with_validator(Box::new(SqlValidator))
        .with_hinter(Box::new(DefaultHinter::default()))
        .with_history(Box::new(history))
        .with_edit_mode(Box::new(Emacs::new(keybindings))))
}

async fn handle_meta(ctx: &mut ShellContext, cmd: &str, args: &[String]) -> Result<bool> {
    match cmd {
        "quit" | "exit" | "q" => return Ok(true),
        "help" | "h" => {
            for (name, desc) in META_COMMANDS {
                eprintln!("  {:<11} {}", name.bold(), desc.dimmed());
            }
            eprintln!(
                "  {}",
                "SQL runs against the current container; `? question` asks the AI".dimmed()
            );
        }
        "profile" => match args.first() {
            Some(name) => match ctx.config.active(Some(name)) {
                Ok((resolved, profile)) => {
                    let resolved = resolved.to_string();
                    let profile = profile.clone();
                    match CosmosClient::new(&profile.account.endpoint).await {
                        Ok(client) => {
                            ctx.profile_name = resolved;
                            ctx.profile = profile;
                            ctx.client = client;
                            ctx.known_databases =
                                ctx.client.list_databases().await.unwrap_or_default();
                            ctx.known_containers = Vec::new();
                        }
                        Err(e) => eprintln!("{} {e:#}", "error:".red().bold()),
                    }
                }
                Err(e) => eprintln!("{} {e}", "error:".red().bold()),
            },
            None => eprintln!(
                "profiles: {}",
                ctx.config
                    .profiles
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        },
        "db" => match args.first() {
            Some(db) => {
                ctx.profile.database = Some(db.clone());
                ctx.profile.container = None;
                ctx.known_containers = ctx.client.list_containers(db).await.unwrap_or_default();
            }
            None => {
                for db in &ctx.known_databases {
                    eprintln!("  {db}");
                }
            }
        },
        "container" => match args.first() {
            Some(c) => ctx.profile.container = Some(c.clone()),
            None => {
                if let Some(db) = ctx.profile.database.clone() {
                    ctx.known_containers =
                        ctx.client.list_containers(&db).await.unwrap_or_default();
                    for c in &ctx.known_containers {
                        eprintln!("  {c}");
                    }
                } else {
                    eprintln!("select a database first (:db <name>)");
                }
            }
        },
        "format" => match args.first().map(String::as_str) {
            Some("json") => ctx.format = OutputFormat::Json,
            Some("json-compact") => ctx.format = OutputFormat::JsonCompact,
            Some("table") => ctx.format = OutputFormat::Table,
            Some("csv") => ctx.format = OutputFormat::Csv,
            _ => eprintln!(
                "formats: json, json-compact, table, csv (current: {:?})",
                ctx.format
            ),
        },
        "queries" => {
            for (name, description) in cosq_core::stored_query::list_query_names() {
                eprintln!(
                    "  {:<24} {}",
                    name.bold(),
                    description.unwrap_or_default().dimmed()
                );
            }
        }
        "run" => match args.first() {
            Some(name) => {
                let params: Vec<String> = args[1..].to_vec();
                if let Err(e) = crate::commands::run::run(crate::commands::run::RunArgs {
                    name: Some(name.clone()),
                    db: ctx.profile.database.clone(),
                    container: ctx.profile.container.clone(),
                    output: Some(ctx.format.clone()),
                    template: None,
                    params,
                    quiet: false,
                })
                .await
                {
                    eprintln!("{} {e:#}", "error:".red().bold());
                }
            }
            None => eprintln!("usage: :run <name> [--param value…]"),
        },
        "schema" | "search" | "explain" => {
            eprintln!("`:{cmd}` arrives with the AI layer of this build");
        }
        other => eprintln!("unknown command :{other} — try :help"),
    }
    Ok(false)
}

async fn handle_ask(_ctx: &mut ShellContext, _question: &str) {
    eprintln!("`? question` arrives with the AI layer of this build");
}

async fn execute_sql(ctx: &mut ShellContext, sql: &str) -> Result<()> {
    let Some(database) = ctx.profile.database.clone() else {
        eprintln!("select a database first (:db <name>)");
        return Ok(());
    };
    let Some(container) = ctx.profile.container.clone() else {
        eprintln!("select a container first (:container <name>)");
        return Ok(());
    };

    // auto-scope to a partition when possible (same logic as `cosq query`)
    let pk_value = match ctx.client.get_container(&database, &container).await {
        Ok(meta) => meta
            .pk_paths
            .first()
            .and_then(|pk| cosq_core::pk_detect::detect_pk_equality(sql, pk, &[])),
        Err(_) => None,
    };
    let opts = QueryOptions::default();
    let result = match &pk_value {
        Some(pk) => {
            eprintln!("{}", format!("scoped to partition {pk}").dimmed());
            ctx.client
                .query_scoped(&database, &container, sql, Vec::new(), pk, &opts)
                .await?
        }
        None => {
            ctx.client
                .query_with_params(&database, &container, sql, Vec::new(), &opts)
                .await?
        }
    };

    let mut stdout = std::io::stdout();
    write_results(&mut stdout, &result.documents, &ctx.format.clone())?;
    eprintln!(
        "{}",
        format!(
            "{} docs · {:.2} RUs",
            result.documents.len(),
            result.request_charge
        )
        .dimmed()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_forms() {
        assert_eq!(classify(""), Input::Empty);
        assert_eq!(
            classify("SELECT * FROM c;"),
            Input::Sql("SELECT * FROM c".into())
        );
        assert_eq!(
            classify("? how many users"),
            Input::Ask("how many users".into())
        );
        assert_eq!(
            classify(":db mydb"),
            Input::Meta("db".into(), vec!["mydb".into()])
        );
        assert_eq!(classify(":HELP"), Input::Meta("help".into(), vec![]));
    }

    #[test]
    fn completion_rules() {
        assert!(is_complete(":db something"));
        assert!(is_complete("? question"));
        assert!(is_complete("SELECT * FROM c;"));
        assert!(is_complete("SELECT * FROM c WHERE c.a = 1"));
        assert!(!is_complete("SELECT * FROM c WHERE c.a = ("));
        assert!(!is_complete("SELECT * FROM c WHERE c.name = 'unterminated"));
        assert!(!is_complete("SELECT *\nFROM c")); // multiline needs `;`
        assert!(is_complete("SELECT *\nFROM c;"));
    }

    #[test]
    fn completer_pools() {
        let c = ShellCompleter {
            metas: vec![":db".into(), ":dbx".into(), ":help".into()],
            databases: vec!["appdb".into(), "analytics".into()],
            containers: vec!["users".into()],
            stored_queries: vec!["recent-users".into()],
            formats: vec!["json".into(), "table".into()],
        };
        assert_eq!(c.candidates(":d"), vec![":db", ":dbx"]);
        assert_eq!(c.candidates(":db a"), vec!["appdb", "analytics"]);
        assert_eq!(c.candidates(":db an"), vec!["analytics"]);
        assert_eq!(c.candidates(":run re"), vec!["recent-users"]);
        assert_eq!(c.candidates(":format t"), vec!["table"]);
        assert!(c.candidates("SELECT ").is_empty());
    }
}
