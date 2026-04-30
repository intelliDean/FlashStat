use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use eyre::Result;
use flashstat_api::FlashApiClient;
use flashstat_common::{FlashBlock, ReorgEvent, SequencerStats, SystemHealth};
use jsonrpsee::http_client::HttpClientBuilder;
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};
use std::{
    io,
    time::{Duration, Instant},
};

struct App {
    blocks: Vec<FlashBlock>,
    reorgs: Vec<ReorgEvent>,
    health: Option<SystemHealth>,
    latest_confidence: f64,
    last_tick: Instant,
    selected_reorg: usize,
    sequencers: Vec<SequencerStats>,
}

impl App {
    fn new() -> App {
        App {
            blocks: Vec::new(),
            reorgs: Vec::new(),
            health: None,
            latest_confidence: 0.0,
            last_tick: Instant::now(),
            selected_reorg: 0,
            sequencers: Vec::new(),
        }
    }

    async fn init(&mut self, client: &(impl FlashApiClient + Sync)) -> Result<()> {
        if let Ok(blocks) = client.get_recent_blocks(50).await {
            self.blocks = blocks;
            if let Some(first) = self.blocks.first() {
                self.latest_confidence = first.confidence;
            }
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
            #[allow(clippy::collapsible_if)]
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

fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(10),
                Constraint::Length(3),
            ]
            .as_ref(),
        )
        .split(f.size());

    // Title / Confidence Gauge
    let status_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)].as_ref())
        .split(chunks[0]);

    let title = Paragraph::new(format!(
        " 🏮 FlashStat Dashboard | Confidence: {:.2}%",
        app.latest_confidence
    ))
    .block(Block::default().borders(Borders::ALL).title("Status"));
    f.render_widget(title, status_chunks[0]);

    let stats_text = if let Some(h) = &app.health {
        format!(
            " Uptime: {}s | Blocks: {} | Alerts: {} ",
            h.uptime_secs, h.total_blocks, h.total_reorgs
        )
    } else {
        " Connecting... ".to_string()
    };
    let stats = Paragraph::new(stats_text)
        .block(Block::default().borders(Borders::ALL).title("System Stats"));
    f.render_widget(stats, status_chunks[1]);

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [
                Constraint::Percentage(40),
                Constraint::Percentage(30),
                Constraint::Percentage(30),
            ]
            .as_ref(),
        )
        .split(chunks[1]);

    // Block Feed
    let blocks: Vec<ListItem> = app
        .blocks
        .iter()
        .map(|b| {
            let content = vec![Line::from(vec![
                Span::styled(
                    format!("#{:<10}", b.number),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw(" | "),
                Span::styled(
                    format!("{:.2}%", b.confidence),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw(" | "),
                Span::raw(format!("{}", b.hash)),
            ])];
            ListItem::new(content)
        })
        .collect();

    let block_list = List::new(blocks).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Live Block Feed"),
    );
    f.render_widget(block_list, main_chunks[0]);

    // Sequencer Reputation
    let sequencers: Vec<ListItem> = app
        .sequencers
        .iter()
        .map(|s| {
            let score_color = if s.reputation_score >= 0 {
                Color::Green
            } else {
                Color::Red
            };
            let content = vec![Line::from(vec![
                Span::styled(
                    format!("{:.4}… ", s.address),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(
                    format!("Score: {:<5}", s.reputation_score),
                    Style::default().fg(score_color),
                ),
            ])];
            ListItem::new(content)
        })
        .collect();

    let sequencer_list = List::new(sequencers).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Sequencer Reputation"),
    );
    f.render_widget(sequencer_list, main_chunks[1]);

    // Reorg Log
    let reorgs: Vec<ListItem> = app
        .reorgs
        .iter()
        .map(|r| {
            let severity_style = match r.severity {
                flashstat_common::ReorgSeverity::Soft => Style::default().fg(Color::Yellow),
                flashstat_common::ReorgSeverity::Deep => {
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                }
                flashstat_common::ReorgSeverity::Equivocation => Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            };

            let content = vec![Line::from(vec![
                Span::styled(format!("{:?}", r.severity), severity_style),
                Span::raw(format!(" at #{}", r.block_number)),
            ])];
            ListItem::new(content)
        })
        .collect();

    let reorg_list = List::new(reorgs)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Security Alerts"),
        )
        .highlight_style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .bg(Color::DarkGray),
        )
        .highlight_symbol(">> ");

    f.render_widget(reorg_list, main_chunks[2]);

    // Analysis Details
    let details_content = if let Some(reorg) = app.reorgs.get(app.selected_reorg) {
        let mut lines = vec![Line::from(vec![
            Span::styled("Event: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format!(
                "{:?} at block #{}",
                reorg.severity, reorg.block_number
            )),
            Span::raw(" | "),
            Span::styled("Detected: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format!("{}", reorg.detected_at.format("%H:%M:%S"))),
        ])];

        if let Some(eq) = &reorg.equivocation {
            lines.push(Line::from(vec![Span::styled(
                "Conflict Analysis:",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            )]));

            if let Some(analysis) = &eq.conflict_analysis {
                lines.push(Line::from(format!(
                    "  Dropped Transactions: {}",
                    analysis.dropped_txs.len()
                )));
                lines.push(Line::from(format!(
                    "  Double Spend Attempts: {}",
                    analysis.double_spend_txs.len()
                )));

                for ds in &analysis.double_spend_txs {
                    lines.push(Line::from(vec![
                        Span::styled("  ⚠️ Double Spend: ", Style::default().fg(Color::Red)),
                        Span::raw(format!("Sender {} | Nonce {}", ds.sender, ds.nonce)),
                    ]));
                    lines.push(Line::from(format!("    TX 1: {}", ds.tx_hash_1)));
                    lines.push(Line::from(format!("    TX 2: {}", ds.tx_hash_2)));
                }
            } else {
                lines.push(Line::from("  (Analysis Pending...)"));
            }
        } else {
            lines.push(Line::from(
                "  No double-spend data available for this event type.",
            ));
        }
        lines
    } else {
        vec![Line::from(
            "Select a security event with Up/Down arrows for details.",
        )]
    };

    let details = Paragraph::new(details_content).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Analysis Forensics (Selected Event)"),
    );
    f.render_widget(details, chunks[2]);

    // Controls
    let help = Paragraph::new(" [q] Quit | [↑/↓] Select Alert | [r] Refresh Proofs ")
        .block(Block::default().borders(Borders::ALL).title("Controls"));
    f.render_widget(help, chunks[3]);
}
