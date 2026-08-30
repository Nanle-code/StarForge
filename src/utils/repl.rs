use crate::utils::history::redact_command;
use anyhow::Result;
use colored::*;
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::history::History;
use rustyline::validate::Validator;
use rustyline::{Context, Editor, Helper};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

pub struct Repl<R>
where
    R: ReplRunner,
{
    runner: R,
    options: ReplOptions,
}

#[derive(Debug, Clone)]
pub struct ReplOptions {
    pub history_enabled: bool,
    pub history_path: PathBuf,
    pub max_history_lines: usize,
    pub completion_candidates: Vec<String>,
    pub contract_methods: Vec<String>,
}

impl Default for ReplOptions {
    fn default() -> Self {
        let mut path = crate::utils::config::config_dir();
        path.push("history");
        Self {
            history_enabled: true,
            history_path: path,
            max_history_lines: 1000,
            completion_candidates: Vec::new(),
            contract_methods: Vec::new(),
        }
    }
}

pub trait ReplRunner {
    fn run_invocation(&mut self, function: &str, args: &[String]) -> Result<String>;
    fn run_simulate(&mut self, function: &str, args: &[String]) -> Result<String> {
        let _ = (function, args);
        Err(anyhow::anyhow!("Simulation not supported"))
    }
    fn run_debug(&mut self, function: &str, args: &[String]) -> Result<String> {
        let _ = (function, args);
        Err(anyhow::anyhow!("Debug execution not supported"))
    }
    fn inspect_state(&mut self, key: Option<&str>) -> Result<String> {
        let _ = key;
        Err(anyhow::anyhow!("State inspection not supported"))
    }
    fn inspect_storage(&mut self, key: &str) -> Result<String> {
        let _ = key;
        Err(anyhow::anyhow!("Storage inspection not supported"))
    }
    fn check_balance(&mut self) -> Result<String> {
        Err(anyhow::anyhow!("Balance check not supported"))
    }
}

impl<R> Repl<R>
where
    R: ReplRunner,
{
    pub fn new(runner: R) -> Self {
        Self {
            runner,
            options: ReplOptions::default(),
        }
    }

    pub fn with_options(runner: R, options: ReplOptions) -> Self {
        Self { runner, options }
    }

    pub fn run(mut self) -> Result<()> {
        println!(
            "  {} {}",
            "StarForge Shell".bright_cyan().bold(),
            "(type :help for commands)".dimmed()
        );

        let mut editor = Editor::<StarForgeHelper, rustyline::history::DefaultHistory>::new()?;
        let all_candidates = self.build_candidates();
        editor.set_helper(Some(StarForgeHelper::new(all_candidates)));
        self.load_history(&mut editor)?;

        loop {
            let prompt = format!("{}", "> ".bright_green().bold());
            let line = match editor.readline(&prompt) {
                Ok(line) => line.trim().to_string(),
                Err(ReadlineError::Interrupted) => continue,
                Err(ReadlineError::Eof) => break,
                Err(err) => return Err(err.into()),
            };
            if line.is_empty() {
                continue;
            }

            if line == ":q" || line == ":quit" || line == ":exit" {
                break;
            }

            if line == ":help" {
                self.print_help();
                continue;
            }

            if let Some(rest) = line.strip_prefix(":history") {
                let rest = rest.trim();
                if rest.is_empty() || rest.starts_with("list") {
                    self.show_history(&editor);
                } else if let Some(term) = rest.strip_prefix("search ") {
                    self.search_history(&editor, term.trim());
                } else if rest == "replay" {
                    self.replay_history(&editor)?;
                } else {
                    eprintln!(
                        "  {} Usage: :history [list|search <term>|replay]",
                        "✗".red().bold()
                    );
                }
                continue;
            }

            if line.starts_with(":state") {
                let key = line
                    .strip_prefix(":state")
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty());
                match self.runner.inspect_state(key) {
                    Ok(out) => println!("{}", out),
                    Err(e) => eprintln!("  {} {}", "✗".red().bold(), e),
                }
                continue;
            }

            if let Some(key) = line.strip_prefix(":storage ") {
                let key = key.trim();
                if key.is_empty() {
                    eprintln!("  {} Usage: :storage <key>", "✗".red().bold());
                } else {
                    match self.runner.inspect_storage(key) {
                        Ok(out) => println!("{}", out),
                        Err(e) => eprintln!("  {} {}", "✗".red().bold(), e),
                    }
                }
                continue;
            }

            if line == ":balance" {
                match self.runner.check_balance() {
                    Ok(out) => println!("{}", out),
                    Err(e) => eprintln!("  {} {}", "✗".red().bold(), e),
                }
                continue;
            }

            if let Some(rest) = line.strip_prefix(":simulate ") {
                let rest = rest.trim();
                if rest.is_empty() {
                    eprintln!(
                        "  {} Usage: :simulate fn(arg1, arg2, ...)",
                        "✗".red().bold()
                    );
                } else {
                    match parse_invocation(rest) {
                        Ok((function, args)) => match self.runner.run_simulate(&function, &args) {
                            Ok(out) => println!("{}", out),
                            Err(e) => eprintln!("  {} {}", "✗".red().bold(), e),
                        },
                        Err(e) => eprintln!("  {} {}", "✗".red().bold(), e),
                    }
                }
                continue;
            }

            if let Some(rest) = line.strip_prefix(":debug ") {
                let rest = rest.trim();
                if rest.is_empty() {
                    eprintln!("  {} Usage: :debug fn(arg1, arg2, ...)", "✗".red().bold());
                } else {
                    match parse_invocation(rest) {
                        Ok((function, args)) => match self.runner.run_debug(&function, &args) {
                            Ok(out) => println!("{}", out),
                            Err(e) => eprintln!("  {} {}", "✗".red().bold(), e),
                        },
                        Err(e) => eprintln!("  {} {}", "✗".red().bold(), e),
                    }
                }
                continue;
            }

            if line == ":trace" {
                println!("  {} Trace mode: enabled for next invocation", "ℹ".cyan());
                // Toggle trace flag for subsequent invocations
                self.push_history(&mut editor, &line)?;
                continue;
            }

            if line.starts_with(":breakpoint ") {
                let fn_name = line
                    .strip_prefix(":breakpoint ")
                    .map(|s| s.trim())
                    .unwrap_or("");
                if fn_name.is_empty() {
                    eprintln!("  {} Usage: :breakpoint <function_name>", "✗".red().bold());
                } else {
                    println!(
                        "  {} Breakpoint set on '{}' (pause before invocation)",
                        "●".yellow().bold(),
                        fn_name
                    );
                }
                continue;
            }

            self.push_history(&mut editor, &line)?;
            match parse_invocation(&line) {
                Ok((function, args)) => match self.runner.run_invocation(&function, &args) {
                    Ok(out) => println!("{}", out),
                    Err(e) => eprintln!("  {} {}", "✗".red().bold(), e),
                },
                Err(e) => eprintln!("  {} {}", "✗".red().bold(), e),
            }
        }

        self.save_history(&mut editor)?;
        Ok(())
    }

    fn build_candidates(&self) -> Vec<String> {
        let mut all = Vec::new();
        all.extend(self.options.completion_candidates.clone());
        all.extend(self.options.contract_methods.clone());
        all.sort();
        all.dedup();
        all
    }

    fn print_help(&self) {
        println!("  {}", "Commands:".bold());
        println!("    :help              Show this help");
        println!("    :quit | :exit      Exit the shell");
        println!("    <TAB>              Auto-complete wallets, contract IDs, and methods");
        println!("    fn(arg1, arg2)     Invoke a contract function");
        println!();
        println!("  {}", "History:".bold().underline());
        println!("    :history           Show command history");
        println!("    :history list      Alias for :history");
        println!("    :history search <term>  Search history for <term>");
        println!("    :history replay   Replay safe commands; confirm side effects");
        println!("    Ctrl+R             Reverse incremental search (built-in)");
        println!();
        println!("  {}", "State Inspection:".bold().underline());
        println!("    :state [key]       Inspect contract state (optionally at a key)");
        println!("    :storage <key>     Inspect a specific storage entry");
        println!("    :balance           Check contract balance");
        println!();
        println!("  {}", "Simulation:".bold().underline());
        println!("    :simulate fn(args)  Simulate a contract invocation (dry-run)");
        println!();
        println!("  {}", "Debugging:".bold().underline());
        println!("    :debug fn(args)    Execute with detailed debug output");
        println!("    :breakpoint <fn>   Set a breakpoint on a function");
        println!("    :trace             Toggle trace mode for next invocation");
    }

    fn show_history(&self, editor: &Editor<StarForgeHelper, rustyline::history::DefaultHistory>) {
        let history = editor.history();
        let entries: Vec<&str> = history.iter().map(|s| s.as_ref()).collect();
        if entries.is_empty() {
            println!("  {} No history entries", "ℹ".cyan());
            return;
        }
        let max_width = (entries.len()).to_string().len();
        for (i, entry) in entries.iter().enumerate() {
            let line = entry.trim();
            if !line.is_empty() {
                println!(
                    "  {num:>width$}  {entry}",
                    num = i + 1,
                    width = max_width,
                    entry = line
                );
            }
        }
    }

    fn search_history(
        &self,
        editor: &Editor<StarForgeHelper, rustyline::history::DefaultHistory>,
        term: &str,
    ) {
        let history = editor.history();
        let entries: Vec<String> = history.iter().map(|s| s.to_string()).collect();
        let matches: Vec<(usize, String)> = entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.contains(term))
            .map(|(i, e)| (i, e.clone()))
            .collect();
        if matches.is_empty() {
            println!("  {} No matches for '{}'", "ℹ".cyan(), term);
            return;
        }
        let max_width = (entries.len()).to_string().len();
        for (i, entry) in &matches {
            println!(
                "  {num:>width$}  {entry}",
                num = i + 1,
                width = max_width,
                entry = entry
            );
        }
    }

    fn replay_history(
        &mut self,
        editor: &Editor<StarForgeHelper, rustyline::history::DefaultHistory>,
    ) -> Result<()> {
        let entries: Vec<String> = editor
            .history()
            .iter()
            .map(|entry| entry.to_string())
            .collect();
        for command in entries {
            let command = command.trim();
            if command.is_empty() || command.contains("[REDACTED]") || command.starts_with(':') {
                continue;
            }

            if is_side_effecting_command(command) {
                println!("  Replay side effect: {} [y/N]", command);
                let mut answer = String::new();
                std::io::stdin().read_line(&mut answer)?;
                if !answer.trim().eq_ignore_ascii_case("y") {
                    continue;
                }
            }

            match parse_invocation(command) {
                Ok((function, args)) => match self.runner.run_invocation(&function, &args) {
                    Ok(output) => println!("{}", output),
                    Err(error) => eprintln!("  {} {}", "✗".red().bold(), error),
                },
                Err(error) => eprintln!("  {} {}", "✗".red().bold(), error),
            }
        }
        Ok(())
    }

    fn load_history(
        &self,
        editor: &mut Editor<StarForgeHelper, rustyline::history::DefaultHistory>,
    ) -> Result<()> {
        if !self.options.history_enabled {
            return Ok(());
        }

        match editor.load_history(&self.options.history_path) {
            Ok(()) => Ok(()),
            Err(ReadlineError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    fn save_history(
        &self,
        editor: &mut Editor<StarForgeHelper, rustyline::history::DefaultHistory>,
    ) -> Result<()> {
        if !self.options.history_enabled {
            return Ok(());
        }

        if let Some(parent) = self.options.history_path.parent() {
            fs::create_dir_all(parent)?;
        }

        editor.save_history(&self.options.history_path)?;
        trim_history_file(&self.options.history_path, self.options.max_history_lines)?;
        Ok(())
    }

    fn push_history(
        &self,
        editor: &mut Editor<StarForgeHelper, rustyline::history::DefaultHistory>,
        line: &str,
    ) -> Result<()> {
        if !self.options.history_enabled {
            return Ok(());
        }
        editor.add_history_entry(redact_command(line))?;
        Ok(())
    }
}

fn trim_history_file(path: &PathBuf, max_lines: usize) -> Result<()> {
    let content = fs::read_to_string(path)?;
    let mut lines = content.lines().map(str::to_string).collect::<Vec<_>>();
    if max_lines == 0 {
        lines.clear();
    } else if lines.len() > max_lines {
        lines = lines.split_off(lines.len() - max_lines);
    }
    fs::write(
        path,
        lines.join("\n") + if lines.is_empty() { "" } else { "\n" },
    )?;
    Ok(())
}

fn is_side_effecting_command(command: &str) -> bool {
    let function = command.split('(').next().unwrap_or(command).trim();
    ["deploy", "invoke", "tx", "upgrade", "migrate"]
        .iter()
        .any(|name| function == *name || function.starts_with(&format!("{}.", name)))
}

#[derive(Clone, Debug)]
struct StarForgeHelper {
    candidates: Vec<String>,
}

impl StarForgeHelper {
    fn new(candidates: Vec<String>) -> Self {
        let mut seen = HashSet::new();
        let mut candidates = candidates
            .into_iter()
            .filter(|candidate| !candidate.trim().is_empty())
            .filter(|candidate| seen.insert(candidate.clone()))
            .collect::<Vec<_>>();
        candidates.sort();
        Self { candidates }
    }
}

impl Helper for StarForgeHelper {}
impl Hinter for StarForgeHelper {
    type Hint = String;
}

impl Highlighter for StarForgeHelper {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> std::borrow::Cow<'l, str> {
        let trimmed = line.trim();
        if trimmed.starts_with(':') {
            if trimmed == ":help" || trimmed == ":quit" || trimmed == ":exit" || trimmed == ":q" {
                return std::borrow::Cow::Owned(format!("{}", line.yellow().bold()));
            }
            if trimmed.starts_with(":history")
                || trimmed.starts_with(":state")
                || trimmed.starts_with(":storage")
                || trimmed.starts_with(":balance")
            {
                return std::borrow::Cow::Owned(format!("{}", line.cyan().bold()));
            }
            if trimmed.starts_with(":simulate") {
                return std::borrow::Cow::Owned(format!("{}", line.magenta().bold()));
            }
            if trimmed.starts_with(":debug")
                || trimmed.starts_with(":breakpoint")
                || trimmed.starts_with(":trace")
            {
                return std::borrow::Cow::Owned(format!("{}", line.red().bold()));
            }
        }

        if let Some(paren) = line.find('(') {
            if paren > 0 {
                let fn_name = &line[..paren];
                let rest = &line[paren..];
                let highlighted_fn = format!("{}", fn_name.trim().cyan().bold());
                let highlighted_rest = highlight_args(rest);
                return std::borrow::Cow::Owned(format!("{}{}", highlighted_fn, highlighted_rest));
            }
        }

        std::borrow::Cow::Borrowed(line)
    }
}

fn highlight_args(input: &str) -> String {
    let mut result = String::new();
    let mut in_string = false;

    for ch in input.chars() {
        if ch == '"' || ch == '\'' {
            in_string = !in_string;
            result.push_str(&format!("{}", ch.to_string().green()));
        } else if in_string {
            result.push_str(&format!("{}", ch.to_string().green()));
        } else {
            result.push(ch);
        }
    }
    result
}

impl Validator for StarForgeHelper {}

impl Completer for StarForgeHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Self::Candidate>)> {
        let start = line[..pos]
            .rfind(|ch: char| ch.is_whitespace() || matches!(ch, '(' | ',' | '"' | '\''))
            .map(|idx| idx + 1)
            .unwrap_or(0);
        let prefix = &line[start..pos];
        let matches = self
            .candidates
            .iter()
            .filter(|candidate| candidate.starts_with(prefix))
            .map(|candidate| Pair {
                display: candidate.clone(),
                replacement: candidate.clone(),
            })
            .collect();
        Ok((start, matches))
    }
}

fn parse_invocation(input: &str) -> Result<(String, Vec<String>)> {
    let open = input
        .find('(')
        .ok_or_else(|| anyhow::anyhow!("Expected invocation like fn(\"arg\")"))?;
    let close = input
        .rfind(')')
        .ok_or_else(|| anyhow::anyhow!("Missing closing ')'"))?;
    if close < open {
        anyhow::bail!("Invalid invocation");
    }

    let function = input[..open].trim();
    if function.is_empty() {
        anyhow::bail!("Missing function name");
    }

    let args_raw = input[open + 1..close].trim();
    let args = split_args(args_raw)?;
    Ok((function.to_string(), args))
}

fn split_args(input: &str) -> Result<Vec<String>> {
    if input.is_empty() {
        return Ok(Vec::new());
    }

    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = '\0';
    let mut escape = false;

    for ch in input.chars() {
        if escape {
            current.push(ch);
            escape = false;
            continue;
        }

        if ch == '\\' {
            escape = true;
            continue;
        }

        if in_quotes {
            if ch == quote_char {
                in_quotes = false;
                continue;
            }
            current.push(ch);
            continue;
        }

        if ch == '"' || ch == '\'' {
            in_quotes = true;
            quote_char = ch;
            continue;
        }

        if ch == ',' {
            args.push(current.trim().to_string());
            current.clear();
            continue;
        }

        current.push(ch);
    }

    if in_quotes {
        anyhow::bail!("Unclosed quote in arguments");
    }

    if escape {
        anyhow::bail!("Trailing escape in arguments");
    }

    args.push(current.trim().to_string());
    Ok(args)
}
