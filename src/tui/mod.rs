use crate::parser::ParsedField;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, HighlightSpacing, List, ListItem, ListState, Paragraph,
    },
    Frame, Terminal,
};
use std::collections::HashMap;
use std::io;

struct TreeItem {
    field: ParsedField,
    depth: usize,
    is_struct: bool,
    children_count: usize,
    collapsed: bool,
    path: String,
}

struct AppState {
    data: Vec<u8>,
    root_field: ParsedField,
    format_name: String,
    hex_offset: usize,
    hex_cursor: usize,
    tree_items: Vec<TreeItem>,
    tree_state: ListState,
    collapsed: HashMap<String, bool>,
    active_panel: Panel,
    search_query: String,
    searching: bool,
    file_size: usize,
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Panel {
    Hex,
    Tree,
}

impl AppState {
    fn new(data: Vec<u8>, root_field: ParsedField, format_name: String) -> Self {
        let file_size = data.len();
        let mut state = Self {
            data,
            root_field: root_field.clone(),
            format_name,
            hex_offset: 0,
            hex_cursor: 0,
            tree_items: Vec::new(),
            tree_state: ListState::default(),
            collapsed: HashMap::new(),
            active_panel: Panel::Hex,
            search_query: String::new(),
            searching: false,
            file_size,
        };
        state.rebuild_tree();
        if !state.tree_items.is_empty() {
            state.tree_state.select(Some(0));
        }
        state
    }

    fn rebuild_tree(&mut self) {
        self.tree_items.clear();
        let root = self.root_field.clone();
        self.build_tree_recursive(&root, 0);
    }

    fn build_tree_recursive(&mut self, field: &ParsedField, depth: usize) {
        let is_struct = matches!(field.value, crate::parser::ParsedValue::Struct(_));
        let collapsed = *self.collapsed.get(&field.path).unwrap_or(&false);
        
        self.tree_items.push(TreeItem {
            field: field.clone(),
            depth,
            is_struct,
            children_count: field.children.len(),
            collapsed,
            path: field.path.clone(),
        });

        if is_struct && !collapsed {
            let children: Vec<ParsedField> = field.children.clone();
            for child in &children {
                self.build_tree_recursive(child, depth + 1);
            }
        }
    }

    fn get_field_at_offset(&self, offset: usize) -> Option<usize> {
        for (idx, item) in self.tree_items.iter().enumerate() {
            if item.field.offset <= offset && offset < item.field.offset + item.field.length {
                if !item.is_struct || item.field.length > 0 && item.field.children.is_empty() {
                    return Some(idx);
                }
            }
        }
        None
    }

    fn get_highlighted_range(&self) -> Option<(usize, usize)> {
        let selected = self.tree_state.selected()?;
        let item = &self.tree_items[selected];
        Some((item.field.offset, item.field.offset + item.field.length))
    }

    fn move_hex_cursor(&mut self, delta: isize) {
        let new_pos = self.hex_cursor as isize + delta;
        if new_pos >= 0 && new_pos < self.file_size as isize {
            self.hex_cursor = new_pos as usize;
            if let Some(tree_idx) = self.get_field_at_offset(self.hex_cursor) {
                self.tree_state.select(Some(tree_idx));
                self.ensure_tree_visible(tree_idx);
            }
        }
    }

    fn move_hex_page(&mut self, forward: bool, height: usize) {
        let bytes_per_page = (height.saturating_sub(2)) * 16;
        let delta = if forward {
            bytes_per_page as isize
        } else {
            -(bytes_per_page as isize)
        };
        self.move_hex_cursor(delta);
    }

    fn move_tree(&mut self, delta: isize) {
        if self.tree_items.is_empty() {
            return;
        }
        let current = self.tree_state.selected().unwrap_or(0);
        let new_pos = current as isize + delta;
        if new_pos >= 0 && new_pos < self.tree_items.len() as isize {
            let new_idx = new_pos as usize;
            self.tree_state.select(Some(new_idx));
            let item = &self.tree_items[new_idx];
            self.hex_cursor = item.field.offset;
            self.ensure_hex_visible();
        }
    }

    fn ensure_tree_visible(&mut self, idx: usize) {
        let selected = self.tree_state.selected().unwrap_or(0);
        if idx != selected {
            self.tree_state.select(Some(idx));
        }
    }

    fn ensure_hex_visible(&mut self) {
        let row = self.hex_cursor / 16;
        let visible_rows = 20;
        if row < self.hex_offset / 16 {
            self.hex_offset = row * 16;
        } else if row >= (self.hex_offset / 16) + visible_rows {
            self.hex_offset = (row - visible_rows + 1) * 16;
        }
    }

    fn toggle_collapse(&mut self) {
        if let Some(selected) = self.tree_state.selected() {
            let item = &self.tree_items[selected];
            if item.is_struct {
                let path = item.path.clone();
                let collapsed = *self.collapsed.get(&path).unwrap_or(&false);
                self.collapsed.insert(path, !collapsed);
                self.rebuild_tree();
                self.tree_state.select(Some(selected));
            }
        }
    }

    fn search(&mut self, query: &str) -> Option<usize> {
        if query.is_empty() {
            return None;
        }
        
        let bytes: Vec<u8> = if query.starts_with("0x") || query.chars().all(|c| c.is_ascii_hexdigit() && c.is_ascii_uppercase() || c.is_ascii_lowercase()) {
            if let Ok(bytes) = hex::decode(query.trim_start_matches("0x")) {
                bytes
            } else {
                query.as_bytes().to_vec()
            }
        } else {
            query.as_bytes().to_vec()
        };

        let query_lower = query.to_lowercase();
        
        let found_in_data = (0..self.data.len().saturating_sub(bytes.len())).find(|&i| {
            self.data[i..i + bytes.len()] == bytes
        });

        if let Some(i) = found_in_data {
            self.hex_cursor = i;
            self.ensure_hex_visible();
            if let Some(tree_idx) = self.get_field_at_offset(i) {
                self.tree_state.select(Some(tree_idx));
            }
            return Some(i);
        }

        let found_in_tree = self.tree_items.iter().enumerate().find(|(_, item)| {
            item.field.name.to_lowercase().contains(&query_lower)
                || item.path.to_lowercase().contains(&query_lower)
        }).map(|(idx, item)| (idx, item.field.offset));

        if let Some((idx, offset)) = found_in_tree {
            self.tree_state.select(Some(idx));
            self.hex_cursor = offset;
            self.ensure_hex_visible();
            return Some(offset);
        }

        None
    }
}

pub fn run_tui(
    data: Vec<u8>,
    root_field: ParsedField,
    format_name: String,
) -> Result<(), io::Error> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let app_state = AppState::new(data, root_field, format_name);
    let res = run_app(&mut terminal, app_state);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("{:?}", err);
    }

    Ok(())
}

fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut state: AppState) -> io::Result<()> {
    loop {
        terminal.draw(|f| {
            ui(f, &mut state);
        })?;

        if let Event::Key(key) = event::read()? {
            if state.searching {
                match key.code {
                    KeyCode::Enter => {
                        state.search(&state.search_query.clone());
                        state.searching = false;
                        state.search_query.clear();
                    }
                    KeyCode::Esc => {
                        state.searching = false;
                        state.search_query.clear();
                    }
                    KeyCode::Backspace => {
                        state.search_query.pop();
                    }
                    KeyCode::Char(c) => {
                        state.search_query.push(c);
                    }
                    _ => {}
                }
                continue;
            }

            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => {
                    return Ok(());
                }
                KeyCode::Tab => {
                    state.active_panel = match state.active_panel {
                        Panel::Hex => Panel::Tree,
                        Panel::Tree => Panel::Hex,
                    };
                }
                KeyCode::Char('/') => {
                    state.searching = true;
                    state.search_query.clear();
                }
                KeyCode::Enter => {
                    if state.active_panel == Panel::Tree {
                        state.toggle_collapse();
                    }
                }
                KeyCode::Up => match state.active_panel {
                    Panel::Hex => state.move_hex_cursor(-16),
                    Panel::Tree => state.move_tree(-1),
                },
                KeyCode::Down => match state.active_panel {
                    Panel::Hex => state.move_hex_cursor(16),
                    Panel::Tree => state.move_tree(1),
                },
                KeyCode::Left => {
                    if state.active_panel == Panel::Hex {
                        state.move_hex_cursor(-1);
                    }
                }
                KeyCode::Right => {
                    if state.active_panel == Panel::Hex {
                        state.move_hex_cursor(1);
                    }
                }
                KeyCode::PageUp => {
                    if state.active_panel == Panel::Hex {
                        state.move_hex_page(false, 20);
                    } else {
                        state.move_tree(-10);
                    }
                }
                KeyCode::PageDown => {
                    if state.active_panel == Panel::Hex {
                        state.move_hex_page(true, 20);
                    } else {
                        state.move_tree(10);
                    }
                }
                KeyCode::Home => {
                    state.hex_cursor = 0;
                    state.hex_offset = 0;
                    if !state.tree_items.is_empty() {
                        state.tree_state.select(Some(0));
                    }
                }
                KeyCode::End => {
                    state.hex_cursor = state.file_size.saturating_sub(1);
                    state.ensure_hex_visible();
                    if !state.tree_items.is_empty() {
                        state.tree_state.select(Some(state.tree_items.len().saturating_sub(1)));
                    }
                }
                _ => {}
            }
        }
    }
}

fn ui(f: &mut Frame, state: &mut AppState) {
    let size = f.size();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(1),
            ]
            .as_ref(),
        )
        .split(size);

    let header = render_header(state);
    f.render_widget(header, chunks[0]);

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(chunks[1]);

    let hex_view = render_hex_view(state);
    f.render_widget(hex_view, main_chunks[0]);

    let tree_view = render_tree_view(&state.tree_items, state.active_panel);
    f.render_stateful_widget(tree_view, main_chunks[1], &mut state.tree_state);

    let footer = render_footer(state);
    f.render_widget(footer, chunks[2]);
}

fn render_header(state: &AppState) -> Paragraph {
    let title = Span::styled(
        format!(" binparse-cli - {} ", state.format_name),
        Style::default()
            .fg(Color::White)
            .bg(Color::Blue)
            .add_modifier(Modifier::BOLD),
    );
    
    let info = Span::styled(
        format!(" File Size: {} bytes (0x{:08X}) | Cursor: 0x{:08X} ", 
            state.file_size, state.file_size, state.hex_cursor),
        Style::default().fg(Color::Gray),
    );

    let text = Text::from(vec![
        Line::from(vec![title]),
        Line::from(vec![info]),
    ]);

    Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Blue)))
        .alignment(Alignment::Left)
}

fn render_hex_view(state: &AppState) -> Paragraph {
    let highlight = state.get_highlighted_range();
    let mut lines = Vec::new();
    let bytes_per_row = 16;
    let visible_rows = 20;
    let start_row = state.hex_offset / bytes_per_row;

    for row in 0..visible_rows {
        let offset = (start_row + row) * bytes_per_row;
        if offset >= state.file_size {
            break;
        }

        let mut spans = Vec::new();
        spans.push(Span::styled(
            format!("{:08X}  ", offset),
            Style::default().fg(Color::DarkGray),
        ));

        for col in 0..bytes_per_row {
            let byte_offset = offset + col;
            if byte_offset >= state.file_size {
                spans.push(Span::styled("   ", Style::default().fg(Color::DarkGray)));
                continue;
            }

            let byte = state.data[byte_offset];
            let is_cursor = byte_offset == state.hex_cursor;
            let is_highlighted = if let Some((start, end)) = highlight {
                byte_offset >= start && byte_offset < end
            } else {
                false
            };

            let style = if is_cursor {
                Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else if is_highlighted {
                Style::default().fg(Color::Black).bg(Color::LightBlue)
            } else {
                Style::default().fg(Color::Green)
            };

            spans.push(Span::styled(format!("{:02X} ", byte), style));

            if col == 7 {
                spans.push(Span::styled(" ", Style::default()));
            }
        }

        spans.push(Span::styled(" |", Style::default().fg(Color::DarkGray)));

        for col in 0..bytes_per_row {
            let byte_offset = offset + col;
            if byte_offset >= state.file_size {
                spans.push(Span::styled(" ", Style::default().fg(Color::DarkGray)));
                continue;
            }

            let byte = state.data[byte_offset];
            let is_cursor = byte_offset == state.hex_cursor;
            let is_highlighted = if let Some((start, end)) = highlight {
                byte_offset >= start && byte_offset < end
            } else {
                false
            };

            let ch = if byte >= 0x20 && byte < 0x7F {
                byte as char
            } else {
                '.'
            };

            let style = if is_cursor {
                Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else if is_highlighted {
                Style::default().fg(Color::Black).bg(Color::LightBlue)
            } else {
                Style::default().fg(Color::White)
            };

            spans.push(Span::styled(format!("{}", ch), style));
        }

        spans.push(Span::styled("|", Style::default().fg(Color::DarkGray)));
        lines.push(Line::from(spans));
    }

    let border_style = if state.active_panel == Panel::Hex {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Gray)
    };

    Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Hex View ")
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .style(Style::default().bg(Color::Black))
}

fn render_tree_view(tree_items: &[TreeItem], active_panel: Panel) -> List {
    let items: Vec<ListItem> = tree_items
        .iter()
        .map(|item| {
            let prefix = "  ".repeat(item.depth);
            let expand_icon = if item.is_struct {
                if item.collapsed {
                    "▶ "
                } else {
                    "▼ "
                }
            } else {
                "  "
            };

            let value_display = if item.field.truncated {
                Span::styled("<truncated>", Style::default().fg(Color::Red))
            } else if item.field.undecidable {
                Span::styled("<undecidable>", Style::default().fg(Color::Yellow))
            } else if item.field.skipped {
                Span::styled("<skipped>", Style::default().fg(Color::Blue))
            } else {
                Span::raw(item.field.value.display(item.field.display_format))
            };

            let checksum_icon = if let Some(result) = &item.field.checksum_result {
                match result {
                    crate::parser::ChecksumResult::Passed => {
                        Span::styled(" ✓", Style::default().fg(Color::Green))
                    }
                    crate::parser::ChecksumResult::Failed { .. } => {
                        Span::styled(" ✗", Style::default().fg(Color::Red))
                    }
                }
            } else {
                Span::raw("")
            };

            let mut spans = vec![
                Span::styled(
                    format!("{}{}{}", prefix, expand_icon, item.field.name),
                    Style::default()
                        .fg(if item.is_struct { Color::Cyan } else { Color::White })
                        .add_modifier(if item.is_struct { Modifier::BOLD } else { Modifier::empty() }),
                ),
                Span::styled(
                    format!(" @ 0x{:08X}[{}] = ", item.field.offset, item.field.length),
                    Style::default().fg(Color::DarkGray),
                ),
                value_display,
                checksum_icon,
            ];

            if item.is_struct && item.children_count > 0 {
                spans.push(Span::styled(
                    format!(" ({} fields)", item.children_count),
                    Style::default().fg(Color::DarkGray),
                ));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    let border_style = if active_panel == Panel::Tree {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Gray)
    };

    List::new(items)
        .block(
            Block::default()
                .title(" Structure Tree ")
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .highlight_style(
            Style::default()
                .bg(Color::LightYellow)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ")
        .highlight_spacing(HighlightSpacing::Always)
}

fn render_footer(state: &AppState) -> Paragraph {
    let text = if state.searching {
        format!("Search: {} (press Enter to search, Esc to cancel)", state.search_query)
    } else {
        "Navigate: ↑↓←→ | Tab: Switch Panel | Enter: Expand/Collapse | /: Search | q: Quit".to_string()
    };

    Paragraph::new(Text::from(text))
        .style(Style::default().fg(Color::Gray).bg(Color::Black))
        .alignment(Alignment::Left)
}
