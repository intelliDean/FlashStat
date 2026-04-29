use flashstat_api::FlashApiClient;
use flashstat_common::FlashBlock;
use flashstat_common::{HealthStatus, ReorgEvent, SequencerStats};
use jsonrpsee::http_client::{HttpClient, HttpClientBuilder};
use ratatui::{
    backend::CrosstermBackend,
    execute,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block as WidgetBlock, Borders, Cell, Paragraph, Row, Table},
    Terminal,
};
use std::{
    io,
    time::{Duration, Instant},
};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use eyre::Result;

struct App {
    blocks: Vec<FlashBlock>,
    reorgs: Vec<ReorgEvent>,
    sequencers: Vec<SequencerStats>,
    health: Option<HealthStatus>,
    last_tick: Instant,
    selected_reorg: usize,
    latest_block: u64,
    latest_confidence: f64,
}

impl App {
    fn new() -> App {
        App {
            blocks: Vec::new(),
            reorgs: Vec::new(),
            sequencers: Vec::new(),
            health: None,
            last_tick: Instant::now(),
            selected_reorg: 0,
            latest_block: 0,
            latest_confidence: 0.0,
        }
    }

    async fn init(&mut self, client: &HttpClient) -> Result<()> {
        if let Ok(Some(block)) = client.get_latest_block().await {
            self.latest_block = block.number;
            self.latest_confidence = block.confidence;
            self.blocks.push(block);
        }
        if let Ok(recent_reorgs) = client.get_recent_reorgs(10).await {
            self.reorgs = recent_reorgs;
        }
        if let Ok(mut sequencers) = client.get_sequencer_rankings().await {
            sequencers.sort_by_key(|s| std::cmp::Reverse(s.reputation_score));
            self.sequencers = sequencers;
        }
        Ok(())
    }

    async fn on_tick(&mut self, client: &(impl FlashApiClient + Sync)) -> Result<()> {
        if let Ok(Some(block)) = client.get_latest_block().await {
            if self.blocks.first().map(|b| b.hash) != Some(block.hash) {
                self.latest_confidence = block.confidence;
                self.blocks.insert(0, block);
                if self.blocks.len() > 50 {
                    self.blocks.pop();
                }
            }
        }

        if let Ok(recent_reorgs) = client.get_recent_reorgs(10).await {
            self.reorgs = recent_reorgs;
        }

        if let Ok(health) = client.get_health().await {
            self.health = Some(health);
        }

        if let Ok(mut sequencers) = client.get_sequencer_rankings().await {
            sequencers.sort_by_key(|s| std::cmp::Reverse(s.reputation_score));
            self.sequencers = sequencers;
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

    let client = HttpClientBuilder::default().build("http://127.0.0.1:9944")?;

    let mut app = App::new();
    let _ = app.init(&client).await;
    let tick_rate = Duration::from_millis(200);

    loop {
        terminal.draw(|f| ui(f, &app))?;

        let timeout = tick_rate
            .checked_sub(app.last_tick.elapsed())
            .unwrap_or_default();

        #[allow(clippy::collapsible_if)]
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Down
                        if !app.reorgs.is_empty() && app.selected_reorg < app.reorgs.len() - 1 =>
                    {
                        app.selected_reorg += 1;
                    }
                    KeyCode::Up if app.selected_reorg > 0 => {
                        app.selected_reorg -= 1;
                    }
                    _ => {}
                }
            }
        }

        if app.last_tick.elapsed() >= tick_rate {
            app.on_tick(&client).await?;
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

fn ui(f: &mut ratatui::Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints(
            [
                Constraint::Length(3),
                Constraint::Min(10),
                Constraint::Length(10),
            ]
            .as_ref(),
        )
        .split(f.size());

    // 1. Header with Health
    let health_text = match &app.health {
        Some(h) => format!(
            "Status: {} | DB Size: {:.2} MB | Total Reorgs: {}",
            h.status,
            h.db_size_bytes as f64 / 1_048_576.0,
            h.total_reorgs
        ),
        None => "Connecting to FlashStat Server...".to_string(),
    };

    let header = Paragraph::new(health_text)
        .style(Style::default().fg(Color::Cyan))
        .block(
            WidgetBlock::default()
                .borders(Borders::ALL)
                .title(" 🏮 FlashStat Dashboard "),
        );
    f.render_widget(header, chunks[0]);

    // 2. Main Content (Blocks and Rankings)
    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)].as_ref())
        .split(chunks[1]);

    // Blocks Table
    let rows: Vec<Row> = app
        .blocks
        .iter()
        .map(|b| {
            let style = if b.confidence > 95.0 {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Yellow)
            };
            Row::new(vec![
                Cell::from(b.number.to_string()),
                Cell::from(format!("{:.2}%", b.confidence)),
                Cell::from(format!("0x{}...", &b.hash.to_string()[2..10])),
            ])
            .style(style)
        })
        .collect();

    let blocks_table = Table::new(
        rows,
        [
            Constraint::Percentage(20),
            Constraint::Percentage(30),
            Constraint::Percentage(50),
        ],
    )
    .header(
        Row::new(vec!["Block", "Confidence", "Hash"])
            .style(Style::default().fg(Color::Gray))
            .bottom_margin(1),
    )
    .block(WidgetBlock::default().borders(Borders::ALL).title(" Recent Blocks "));
    f.render_widget(blocks_table, body_chunks[0]);

    // Rankings Table
    let ranking_rows: Vec<Row> = app
        .sequencers
        .iter()
        .map(|s| {
            Row::new(vec![
                Cell::from(format!("0x{}...", &s.address.to_string()[2..8])),
                Cell::from(s.reputation_score.to_string()),
                Cell::from(s.total_attested_blocks.to_string()),
            ])
        })
        .collect();

    let rankings_table = Table::new(
        ranking_rows,
        [
            Constraint::Percentage(40),
            Constraint::Percentage(30),
            Constraint::Percentage(30),
        ],
    )
    .header(
        Row::new(vec!["Sequencer", "Score", "TEE Docs"])
            .style(Style::default().fg(Color::Gray))
            .bottom_margin(1),
    )
    .block(WidgetBlock::default().borders(Borders::ALL).title(" Reputation Ranking "));
    f.render_widget(rankings_table, body_chunks[1]);

    // 3. Reorgs/Equivocations
    let reorg_rows: Vec<Row> = app
        .reorgs
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let style = if i == app.selected_reorg {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::LightRed)
            };
            Row::new(vec![
                Cell::from(r.block_number.to_string()),
                Cell::from(format!("{:?}", r.severity)),
                Cell::from(r.detected_at.format("%H:%M:%S").to_string()),
            ])
            .style(style)
        })
        .collect();

    let reorgs_table = Table::new(
        reorg_rows,
        [
            Constraint::Percentage(30),
            Constraint::Percentage(40),
            Constraint::Percentage(30),
        ],
    )
    .header(
        Row::new(vec!["Block", "Type", "Detected At"])
            .style(Style::default().fg(Color::Gray))
            .bottom_margin(1),
    )
    .block(
        WidgetBlock::default()
            .borders(Borders::ALL)
            .title(" 🚨 Security Alerts (Equivocations) "),
    );
    f.render_widget(reorgs_table, chunks[2]);
}
