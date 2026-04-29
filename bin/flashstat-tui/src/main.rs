use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use flashstat_api::FlashApiClient;
use flashstat_common::{FlashBlock, ReorgEvent};
use jsonrpsee::http_client::HttpClientBuilder;
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Span, Spans},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph},
    Frame, Terminal,
};
use std::{io, time::{Duration, Instant}};
use eyre::Result;

struct App {
    blocks: Vec<FlashBlock>,
    reorgs: Vec<ReorgEvent>,
    latest_confidence: f64,
    last_tick: Instant,
}

impl App {
    fn new() -> App {
        App {
            blocks: Vec::new(),
            reorgs: Vec::new(),
            latest_confidence: 0.0,
            last_tick: Instant::now(),
        }
    }

    async fn on_tick(&mut self, client: &impl FlashApiClient) -> Result<()> {
        if let Ok(Some(block)) = client.get_latest_block().await {
            if self.blocks.last().map(|b| b.hash) != Some(block.hash) {
                self.latest_confidence = block.confidence;
                self.blocks.push(block);
                if self.blocks.len() > 50 {
                    self.blocks.remove(0);
                }
            }
        }
        
        if let Ok(recent_reorgs) = client.get_recent_reorgs(10).await {
            self.reorgs = recent_reorgs;
        }
        
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let client = HttpClientBuilder::default()
        .build("http://127.0.0.1:9944")?;

    let mut app = App::new();
    let tick_rate = Duration::from_millis(200);
    
    loop {
        terminal.draw(|f| ui(f, &app))?;

        let timeout = tick_rate
            .checked_sub(app.last_tick.elapsed())
            .unwrap_or_default();
            
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if let KeyCode::Char('q') = key.code {
                    break;
                }
            }
        }

        if app.last_tick.elapsed() >= tick_rate {
            let _ = app.on_tick(&client).await;
            app.last_tick = Instant::now();
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

fn ui<B: Backend>(f: &mut Frame<B>, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(10),
            ]
            .as_ref(),
        )
        .split(f.size());

    // Title / Confidence Gauge
    let title = Paragraph::new(format!(" 🏮 FlashStat Dashboard | Confidence: {:.2}%", app.latest_confidence))
        .block(Block::default().borders(Borders::ALL).title("Status"));
    f.render_widget(title, chunks[0]);

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)].as_ref())
        .split(chunks[1]);

    // Block Feed
    let blocks: Vec<ListItem> = app.blocks.iter().rev().map(|b| {
        let content = vec![Spans::from(vec![
            Span::styled(format!("#{:<10}", b.number), Style::default().fg(Color::Cyan)),
            Span::raw(" | "),
            Span::styled(format!("{:.2}%", b.confidence), Style::default().fg(Color::Yellow)),
            Span::raw(" | "),
            Span::raw(format!("{}", b.hash)),
        ])];
        ListItem::new(content)
    }).collect();
    
    let block_list = List::new(blocks)
        .block(Block::default().borders(Borders::ALL).title("Live Block Feed"));
    f.render_widget(block_list, main_chunks[0]);

    // Reorg Log
    let reorgs: Vec<ListItem> = app.reorgs.iter().map(|r| {
        let severity_style = match r.severity {
            flashstat_common::ReorgSeverity::Soft => Style::default().fg(Color::Yellow),
            flashstat_common::ReorgSeverity::Deep => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            flashstat_common::ReorgSeverity::Equivocation => Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
        };
        
        let content = vec![Spans::from(vec![
            Span::styled(format!("{:?}", r.severity), severity_style),
            Span::raw(format!(" at #{}", r.block_number)),
        ])];
        ListItem::new(content)
    }).collect();

    let reorg_list = List::new(reorgs)
        .block(Block::default().borders(Borders::ALL).title("Security Alerts"));
    f.render_widget(reorg_list, main_chunks[1]);

    // Controls
    let help = Paragraph::new(" [q] Quit | [r] Refresh Proofs ")
        .block(Block::default().borders(Borders::ALL).title("Controls"));
    f.render_widget(help, chunks[2]);
}
