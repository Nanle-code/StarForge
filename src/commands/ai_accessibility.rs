//! CLI for AI Accessibility Features (issue #521).

use crate::utils::ai_accessibility as a11y;
use crate::utils::ollama;
use crate::utils::print as p;
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use std::io::{self, Read};

#[derive(Subcommand)]
pub enum AiAccessibilityCommands {
    /// Show current accessibility configuration
    Status {
        #[arg(long)]
        json: bool,
    },

    /// Configure accessibility settings
    Configure(ConfigureArgs),

    /// Simplify text for easier reading
    Simplify(SimplifyArgs),

    /// Format text for screen readers
    ScreenReader(ScreenReaderArgs),

    /// List or execute voice commands
    #[command(subcommand)]
    Voice(VoiceCommands),

    /// List keyboard shortcuts
    Shortcuts {
        #[arg(long)]
        json: bool,

        /// Filter by category
        #[arg(long)]
        category: Option<String>,
    },

    /// Toggle or set high contrast mode
    HighContrast {
        /// Explicitly enable or disable
        #[arg(long)]
        enable: Option<bool>,
    },

    /// Toggle screen reader mode
    ScreenReaderMode {
        #[arg(long)]
        enable: Option<bool>,
    },

    /// Toggle simplified text mode
    SimplifiedText {
        #[arg(long)]
        enable: Option<bool>,
    },

    /// Check text for WCAG compliance
    WcagCheck(WcagCheckArgs),

    /// Format text according to active accessibility settings
    Format(FormatArgs),
}

#[derive(Args)]
pub struct ConfigureArgs {
    #[arg(long)]
    pub screen_reader: Option<bool>,

    #[arg(long)]
    pub simplified_text: Option<bool>,

    #[arg(long)]
    pub high_contrast: Option<bool>,

    #[arg(long)]
    pub voice_commands: Option<bool>,

    #[arg(long)]
    pub keyboard_shortcuts: Option<bool>,

    #[arg(long)]
    pub reduce_motion: Option<bool>,

    #[arg(long)]
    pub announce_progress: Option<bool>,

    #[arg(long)]
    pub verbose_descriptions: Option<bool>,

    /// Font size: small, medium, large, extra_large
    #[arg(long)]
    pub font_size: Option<String>,
}

#[derive(Args)]
pub struct SimplifyArgs {
    /// Text to simplify (reads from stdin if omitted)
    pub text: Option<String>,

    #[arg(long, default_value_t = false)]
    pub use_ai: bool,

    #[arg(long, default_value = ollama::DEFAULT_MODEL)]
    pub model: String,
}

#[derive(Args)]
pub struct ScreenReaderArgs {
    /// Text to format (reads from stdin if omitted)
    pub text: Option<String>,

    /// Section title
    #[arg(long, default_value = "Output")]
    pub title: String,
}

#[derive(Subcommand)]
pub enum VoiceCommands {
    /// List all available voice commands
    List {
        #[arg(long)]
        json: bool,

        #[arg(long)]
        category: Option<String>,
    },

    /// Match a spoken phrase to a command
    Match {
        /// Spoken phrase to match
        phrase: String,

        #[arg(long)]
        json: bool,
    },
}

#[derive(Args)]
pub struct WcagCheckArgs {
    /// Text to check (reads from stdin if omitted)
    pub text: Option<String>,

    /// WCAG level: a, aa, aaa
    #[arg(long, default_value = "aa")]
    pub level: String,

    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct FormatArgs {
    /// Text to format (reads from stdin if omitted)
    pub text: Option<String>,
}

pub async fn handle(cmd: AiAccessibilityCommands) -> Result<()> {
    match cmd {
        AiAccessibilityCommands::Status { json } => handle_status(json),
        AiAccessibilityCommands::Configure(args) => handle_configure(args),
        AiAccessibilityCommands::Simplify(args) => handle_simplify(args).await,
        AiAccessibilityCommands::ScreenReader(args) => handle_screen_reader(args),
        AiAccessibilityCommands::Voice(voice) => handle_voice(voice),
        AiAccessibilityCommands::Shortcuts { json, category } => {
            handle_shortcuts(json, category.as_deref())
        }
        AiAccessibilityCommands::HighContrast { enable } => handle_toggle("high_contrast", enable),
        AiAccessibilityCommands::ScreenReaderMode { enable } => {
            handle_toggle("screen_reader", enable)
        }
        AiAccessibilityCommands::SimplifiedText { enable } => {
            handle_toggle("simplified_text", enable)
        }
        AiAccessibilityCommands::WcagCheck(args) => handle_wcag_check(args),
        AiAccessibilityCommands::Format(args) => handle_format(args),
    }
}

fn handle_status(json: bool) -> Result<()> {
    let cfg = a11y::load_config()?;

    if json {
        println!("{}", serde_json::to_string_pretty(&cfg)?);
        return Ok(());
    }

    p::header("Accessibility Configuration");
    p::separator();
    p::kv("Screen reader mode", &cfg.screen_reader_mode.to_string());
    p::kv(
        "Simplified text mode",
        &cfg.simplified_text_mode.to_string(),
    );
    p::kv("High contrast mode", &cfg.high_contrast_mode.to_string());
    p::kv("Voice commands", &cfg.voice_commands_enabled.to_string());
    p::kv(
        "Keyboard shortcuts",
        &cfg.keyboard_shortcuts_enabled.to_string(),
    );
    p::kv("Reduce motion", &cfg.reduce_motion.to_string());
    p::kv("Announce progress", &cfg.announce_progress.to_string());
    p::kv(
        "Verbose descriptions",
        &cfg.verbose_descriptions.to_string(),
    );
    p::kv("Font size", &format!("{:?}", cfg.font_size));
    p::separator();
    Ok(())
}

fn handle_configure(args: ConfigureArgs) -> Result<()> {
    let cfg = a11y::update_config(|c| {
        if let Some(v) = args.screen_reader {
            c.screen_reader_mode = v;
        }
        if let Some(v) = args.simplified_text {
            c.simplified_text_mode = v;
        }
        if let Some(v) = args.high_contrast {
            c.high_contrast_mode = v;
        }
        if let Some(v) = args.voice_commands {
            c.voice_commands_enabled = v;
        }
        if let Some(v) = args.keyboard_shortcuts {
            c.keyboard_shortcuts_enabled = v;
        }
        if let Some(v) = args.reduce_motion {
            c.reduce_motion = v;
        }
        if let Some(v) = args.announce_progress {
            c.announce_progress = v;
        }
        if let Some(v) = args.verbose_descriptions {
            c.verbose_descriptions = v;
        }
        if let Some(ref size) = args.font_size {
            c.font_size = parse_font_size(size).unwrap_or(c.font_size);
        }
    })?;

    p::success("Accessibility settings updated.");
    if !args.screen_reader.is_none()
        || !args.simplified_text.is_none()
        || !args.high_contrast.is_none()
    {
        p::kv("Screen reader", &cfg.screen_reader_mode.to_string());
        p::kv("Simplified text", &cfg.simplified_text_mode.to_string());
        p::kv("High contrast", &cfg.high_contrast_mode.to_string());
    }
    Ok(())
}

fn parse_font_size(s: &str) -> Result<a11y::FontSize> {
    match s.to_lowercase().as_str() {
        "small" => Ok(a11y::FontSize::Small),
        "medium" => Ok(a11y::FontSize::Medium),
        "large" => Ok(a11y::FontSize::Large),
        "extra_large" | "extra-large" | "xlarge" => Ok(a11y::FontSize::ExtraLarge),
        other => anyhow::bail!(
            "Unknown font size '{}'. Use: small, medium, large, extra_large",
            other
        ),
    }
}

async fn handle_simplify(args: SimplifyArgs) -> Result<()> {
    let text = read_text_input(args.text.as_deref())?;

    p::header("Text Simplification");
    p::separator();

    let simplified = if args.use_ai {
        if !ollama::is_ollama_running().await {
            p::warn("Ollama not running — using local simplification.");
            a11y::simplify_text(&text, false, &args.model).await?
        } else {
            let spinner = p::spinner("Simplifying with AI…");
            let result = a11y::simplify_text(&text, true, &args.model).await?;
            spinner.finish_and_clear();
            result
        }
    } else {
        a11y::simplify_text_local(&text)
    };

    println!("{}", simplified);
    p::separator();
    Ok(())
}

fn handle_screen_reader(args: ScreenReaderArgs) -> Result<()> {
    let text = read_text_input(args.text.as_deref())?;
    let cfg = a11y::load_config()?;
    let mut cfg = cfg;
    cfg.screen_reader_mode = true;

    let formatted = a11y::screen_reader_format(&text, &cfg);
    let output = a11y::format_for_screen_reader(&args.title, &[("Content".into(), formatted)]);

    p::header("Screen Reader Output");
    p::separator();
    println!("{}", output.content);
    p::separator();
    Ok(())
}

fn handle_voice(cmd: VoiceCommands) -> Result<()> {
    match cmd {
        VoiceCommands::List { json, category } => {
            let filter_cat = category.clone();
            let commands = if let Some(ref cat) = filter_cat {
                a11y::voice_commands()
                    .into_iter()
                    .filter(|c| c.category == *cat)
                    .collect()
            } else {
                a11y::voice_commands()
            };

            if json {
                println!("{}", serde_json::to_string_pretty(&commands)?);
                return Ok(());
            }

            p::header("Voice Commands");
            p::separator();
            let by_cat = a11y::voice_commands_by_category();
            for (cat, cmds) in &by_cat {
                if filter_cat.as_ref().is_some_and(|c| c != cat) {
                    continue;
                }
                println!();
                p::info(&format!("Category: {}", cat));
                for cmd in cmds {
                    println!("  \"{}\" → {}", cmd.phrase, cmd.action);
                    if !cmd.aliases.is_empty() {
                        println!("    Aliases: {}", cmd.aliases.join(", "));
                    }
                }
            }
            p::separator();
            Ok(())
        }
        VoiceCommands::Match { phrase, json } => {
            let m = a11y::match_voice_command(&phrase)
                .with_context(|| format!("No voice command matched: \"{}\"", phrase))?;

            if json {
                println!("{}", serde_json::to_string_pretty(&m)?);
                return Ok(());
            }

            p::header("Voice Command Match");
            p::separator();
            p::kv("Matched phrase", &m.matched_phrase);
            p::kv("Confidence", &format!("{:.0}%", m.confidence * 100.0));
            p::kv("Action", &m.command.action);
            p::kv("Description", &m.command.description);
            p::separator();
            Ok(())
        }
    }
}

fn handle_shortcuts(json: bool, category: Option<&str>) -> Result<()> {
    let shortcuts: Vec<_> = if let Some(cat) = category {
        a11y::keyboard_shortcuts()
            .into_iter()
            .filter(|s| s.category == cat)
            .collect()
    } else {
        a11y::keyboard_shortcuts()
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&shortcuts)?);
        return Ok(());
    }

    p::header("Keyboard Shortcuts");
    p::separator();
    let headers = &["Keys", "Action", "Description", "Category"];
    let rows: Vec<Vec<String>> = shortcuts
        .iter()
        .map(|s| {
            vec![
                s.keys.clone(),
                s.action.clone(),
                s.description.clone(),
                s.category.clone(),
            ]
        })
        .collect();
    p::table(headers, &rows);
    p::separator();
    Ok(())
}

fn handle_toggle(setting: &str, enable: Option<bool>) -> Result<()> {
    let cfg = a11y::update_config(|c| {
        let new_val = enable.unwrap_or(match setting {
            "high_contrast" => !c.high_contrast_mode,
            "screen_reader" => !c.screen_reader_mode,
            "simplified_text" => !c.simplified_text_mode,
            _ => false,
        });

        match setting {
            "high_contrast" => c.high_contrast_mode = new_val,
            "screen_reader" => c.screen_reader_mode = new_val,
            "simplified_text" => c.simplified_text_mode = new_val,
            _ => {}
        }
    })?;

    let label = setting.replace('_', " ");
    let state = match setting {
        "high_contrast" => cfg.high_contrast_mode,
        "screen_reader" => cfg.screen_reader_mode,
        "simplified_text" => cfg.simplified_text_mode,
        _ => false,
    };

    p::success(&format!(
        "{} mode: {}",
        label,
        if state { "enabled" } else { "disabled" }
    ));
    Ok(())
}

fn handle_wcag_check(args: WcagCheckArgs) -> Result<()> {
    let text = read_text_input(args.text.as_deref())?;
    let level = parse_wcag_level(&args.level)?;
    let result = a11y::check_wcag_compliance(&text, level);

    if args.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    p::header("WCAG Compliance Check");
    p::separator();
    p::kv("Level", &format!("{:?}", result.level));
    p::kv("Score", &format!("{}%", result.score_pct));
    p::kv("Passed", &result.passed.to_string());

    if !result.violations.is_empty() {
        println!();
        p::warn("Violations:");
        for v in &result.violations {
            println!("  [{}] {} — {}", v.criterion, v.description, v.fix);
        }
    }

    if !result.recommendations.is_empty() {
        println!();
        p::info("Recommendations:");
        for r in &result.recommendations {
            println!("  • {}", r);
        }
    }

    p::separator();
    Ok(())
}

fn handle_format(args: FormatArgs) -> Result<()> {
    let text = read_text_input(args.text.as_deref())?;
    let cfg = a11y::load_config()?;
    let formatted = a11y::format_output(&text, &cfg);
    println!("{}", formatted);
    Ok(())
}

fn parse_wcag_level(s: &str) -> Result<a11y::WcagLevel> {
    match s.to_lowercase().as_str() {
        "a" => Ok(a11y::WcagLevel::A),
        "aa" => Ok(a11y::WcagLevel::Aa),
        "aaa" => Ok(a11y::WcagLevel::Aaa),
        other => anyhow::bail!("Unknown WCAG level '{}'. Use: a, aa, aaa", other),
    }
}

fn read_text_input(arg: Option<&str>) -> Result<String> {
    if let Some(text) = arg {
        return Ok(text.to_string());
    }
    let mut buf = String::new();
    io::stdin()
        .read_to_string(&mut buf)
        .context("Failed to read from stdin")?;
    if buf.trim().is_empty() {
        anyhow::bail!("No text provided. Pass text as an argument or pipe via stdin.");
    }
    Ok(buf)
}
