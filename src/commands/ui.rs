use clap::Parser;
use std::io;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Span, Spans},
    widgets::{Block, Borders, Paragraph, Clear, List, ListItem, ListState, Tabs},
    Terminal,
};
use crate::utils;

#[derive(Parser, Debug)]
pub struct UiArgs {
}

pub async fn handle(_args: UiArgs) -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::default();

    let res = run_app(&mut terminal, &mut app).await;

    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{:?}", err);
    }

    Ok(())
}

struct App {
    active_tab: usize,
    show_help: bool,
    should_quit: bool,
    wallets: Vec<String>,
    contracts: Vec<String>,
    transactions: Vec<String>,
    events: Vec<String>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            active_tab: 0,
            show_help: false,
            should_quit: false,
            wallets: vec!["Alice (Testnet) - 100 XLM".into(), "Bob (Testnet) - 50 XLM".into()],
            contracts: vec!["Token (v1.0.0, TTL: 10000)".into(), "Swap (v2.1.0, TTL: 4500)".into()],
            transactions: vec!["Tx1: Transfer 10 XLM".into(), "Tx2: Deploy Token".into()],
            events: vec!["Event: Token Mint".into(), "Event: Swap executed".into()],
        }
    }
}

async fn run_app<B: ratatui::backend::Backend>(terminal: &mut Terminal<B>, app: &mut App) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;

        if event::poll(std::time::Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
                    app.should_quit = true;
                } else {
                    match key.code {
                        KeyCode::Char('q') => app.should_quit = true,
                        KeyCode::Char('?') | KeyCode::Char('h') => app.show_help = !app.show_help,
                        KeyCode::Esc => app.show_help = false,
                        KeyCode::Right | KeyCode::Tab => {
                            if !app.show_help {
                                app.active_tab = (app.active_tab + 1) % 4;
                            }
                        }
                        KeyCode::Left => {
                            if !app.show_help {
                                app.active_tab = (app.active_tab + 3) % 4;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn ui<B: ratatui::backend::Backend>(f: &mut ratatui::Frame<B>, app: &App) {
    let size = f.size();
    
    // Check NO_COLOR and plain mode (accessibility)
    let no_color = utils::output::is_plain_mode_enabled();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ].as_ref())
        .split(size);

    let titles = vec!["Wallets", "Contracts", "Transactions", "Events"]
        .into_iter()
        .map(|t| {
            if no_color {
                ratatui::text::Line::from(t)
            } else {
                ratatui::text::Line::from(Span::styled(t, Style::default().fg(Color::Green)))
            }
        })
        .collect::<Vec<_>>();

    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title("StarForge UI"))
        .select(app.active_tab)
        .style(Style::default())
        .highlight_style(if no_color {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        });
    f.render_widget(tabs, chunks[0]);

    let content_block = Block::default().borders(Borders::ALL);
    match app.active_tab {
        0 => {
            let items: Vec<ListItem> = app.wallets.iter().map(|i| ListItem::new(i.clone())).collect();
            let list = List::new(items).block(content_block.title("Wallets"));
            f.render_widget(list, chunks[1]);
        }
        1 => {
            let items: Vec<ListItem> = app.contracts.iter().map(|i| ListItem::new(i.clone())).collect();
            let list = List::new(items).block(content_block.title("Contracts"));
            f.render_widget(list, chunks[1]);
        }
        2 => {
            let items: Vec<ListItem> = app.transactions.iter().map(|i| ListItem::new(i.clone())).collect();
            let list = List::new(items).block(content_block.title("Transactions"));
            f.render_widget(list, chunks[1]);
        }
        3 => {
            let items: Vec<ListItem> = app.events.iter().map(|i| ListItem::new(i.clone())).collect();
            let list = List::new(items).block(content_block.title("Live Events"));
            f.render_widget(list, chunks[1]);
        }
        _ => {}
    };

    let help_msg = "Press '?' or 'h' for help | 'q' to quit";
    let status_bar = Paragraph::new(help_msg).style(if no_color { Style::default() } else { Style::default().fg(Color::DarkGray) });
    f.render_widget(status_bar, chunks[2]);

    if app.show_help {
        let area = centered_rect(60, 20, size);
        f.render_widget(Clear, area); // clear the background
        let help_text = vec![
            ratatui::text::Line::from("StarForge UI Help"),
            ratatui::text::Line::from(""),
            ratatui::text::Line::from("Navigation:"),
            ratatui::text::Line::from("  Left/Right, Tab  : Switch panes"),
            ratatui::text::Line::from("  q                : Quit"),
            ratatui::text::Line::from("  ?, h             : Toggle this help"),
            ratatui::text::Line::from("  Esc              : Close help"),
        ];
        let block = Block::default().title("Help").borders(Borders::ALL).style(if no_color { Style::default() } else { Style::default().fg(Color::Cyan) });
        let paragraph = Paragraph::new(help_text).block(block);
        f.render_widget(paragraph, area);
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ].as_ref())
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ].as_ref())
        .split(popup_layout[1])[1]
}
