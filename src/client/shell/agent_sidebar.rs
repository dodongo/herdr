use std::collections::HashMap;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Paragraph, Widget},
};

use super::*;
use crate::protocol::{ClientShellAgent, ClientShellPane};

pub(super) struct AgentRow {
    pub(super) pane_id: String,
    pub(super) status: Option<crate::api::schema::AgentStatus>,
    pub(super) focused: bool,
    pub(super) depth: usize,
    pub(super) rows: Vec<Vec<crate::ui::ResolvedToken>>,
}

pub(super) fn ordered_agent_pane_ids(snapshot: &ClientShellSnapshot) -> Vec<String> {
    agent_list_entries(snapshot, ordered_agents(snapshot))
        .into_iter()
        .map(|(entry, _)| entry.pane_id().to_owned())
        .collect()
}

#[derive(Clone, Copy)]
pub(super) enum AgentListEntry<'a> {
    Agent(&'a ClientShellAgent),
    Pane(&'a ClientShellPane),
}

impl<'a> AgentListEntry<'a> {
    pub(super) fn pane_id(self) -> &'a str {
        match self {
            Self::Agent(agent) => &agent.pane_id,
            Self::Pane(pane) => &pane.pane_id,
        }
    }
}

pub(super) fn agent_list_entries<'a>(
    snapshot: &'a ClientShellSnapshot,
    agents: Vec<(&'a ClientShellAgent, usize)>,
) -> Vec<(AgentListEntry<'a>, usize)> {
    let agent_ids = snapshot
        .agents
        .iter()
        .map(|agent| agent.pane_id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let mut panes_by_parent: HashMap<&str, Vec<&ClientShellPane>> = HashMap::new();
    for pane in &snapshot.panes {
        if agent_ids.contains(pane.pane_id.as_str())
            || !pane.tokens.iter().any(|(key, value)| {
                key == crate::metadata_tokens::PRESENTATION_TOKEN && value == "pane"
            })
        {
            continue;
        }
        if let Some((_, parent)) = pane.tokens.iter().find(|(key, _)| key == "p_parent_pane") {
            panes_by_parent.entry(parent).or_default().push(pane);
        }
    }
    let mut entries = Vec::with_capacity(agents.len());
    let mut pending = Vec::new();
    for (agent, depth) in agents {
        while pending
            .last()
            .is_some_and(|(_, parent_depth)| *parent_depth >= depth)
        {
            if let Some((pane, parent_depth)) = pending.pop() {
                entries.push((AgentListEntry::Pane(pane), parent_depth + 1));
            }
        }
        entries.push((AgentListEntry::Agent(agent), depth));
        if let Some(panes) = panes_by_parent.remove(agent.pane_id.as_str()) {
            pending.extend(panes.into_iter().rev().map(|pane| (pane, depth)));
        }
    }
    entries.extend(
        pending
            .into_iter()
            .rev()
            .map(|(pane, depth)| (AgentListEntry::Pane(pane), depth + 1)),
    );
    entries
}

pub(super) fn ordered_agents(snapshot: &ClientShellSnapshot) -> Vec<(&ClientShellAgent, usize)> {
    if snapshot.agent_view_label.is_some() {
        return snapshot
            .agent_order
            .iter()
            .filter_map(|pane_id| {
                snapshot
                    .agents
                    .iter()
                    .find(|agent| agent.pane_id == *pane_id)
                    .map(|agent| (agent, 0))
            })
            .collect();
    }
    tree_ordered_agents(snapshot.agents.iter())
}

pub(super) fn tree_ordered_agents<'a>(
    agents: impl IntoIterator<Item = &'a ClientShellAgent>,
) -> Vec<(&'a ClientShellAgent, usize)> {
    let agents = agents.into_iter().collect::<Vec<_>>();
    let indices = agents
        .iter()
        .enumerate()
        .map(|(index, agent)| (agent.pane_id.as_str(), index))
        .collect::<HashMap<_, _>>();
    let mut parents = agents
        .iter()
        .map(|agent| {
            agent
                .tokens
                .iter()
                .find(|(key, _)| key == "p_parent_pane")
                .and_then(|(_, parent)| indices.get(parent.as_str()).copied())
        })
        .collect::<Vec<_>>();
    let mut visited = vec![false; agents.len()];
    for start in 0..agents.len() {
        let mut path = Vec::new();
        let mut current = Some(start);
        while let Some(index) = current {
            if visited[index] {
                if let Some(cycle_start) = path.iter().position(|ancestor| *ancestor == index) {
                    for ancestor in &path[cycle_start..] {
                        parents[*ancestor] = None;
                    }
                }
                break;
            }
            visited[index] = true;
            path.push(index);
            current = parents[index];
        }
    }
    let mut children = vec![Vec::new(); agents.len()];
    let mut roots = Vec::new();
    for (index, parent) in parents.into_iter().enumerate() {
        if let Some(parent) = parent {
            children[parent].push(index);
        } else {
            roots.push(index);
        }
    }
    let mut pending = roots
        .into_iter()
        .rev()
        .map(|index| (index, 0))
        .collect::<Vec<_>>();
    let mut ordered = Vec::with_capacity(agents.len());
    while let Some((index, depth)) = pending.pop() {
        ordered.push((agents[index], depth));
        pending.extend(
            children[index]
                .iter()
                .rev()
                .map(|child| (*child, depth + 1)),
        );
    }
    ordered
}

pub(super) fn render_agent_panel(
    buffer: &mut Buffer,
    area: Rect,
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    agent_scroll: &mut usize,
    hits: &mut ShellHitMap,
) {
    if !render_agent_panel_header(buffer, area, snapshot.agent_view_label.as_deref(), config) {
        return;
    }

    let rows = agent_rows(snapshot, config, None);
    render_agent_list(
        buffer,
        area,
        &rows,
        snapshot
            .agent_view_label
            .as_ref()
            .map(|_| " no matching agents"),
        config,
        agent_scroll,
        hits,
        |row| row.rows.len(),
        |buffer, rect, row, hits| {
            hits.agents.push((rect, row.pane_id.clone()));
            render_agent_row(buffer, rect, row, config);
        },
    );
}

pub(super) fn render_agent_panel_header(
    buffer: &mut Buffer,
    area: Rect,
    agent_view_label: Option<&str>,
    config: &ClientShellConfig,
) -> bool {
    if area.height == 0 {
        return false;
    }
    put_text(
        buffer,
        area.x,
        area.y,
        area.width,
        &"─".repeat(area.width as usize),
        Style::default().fg(config.palette.surface_dim),
    );
    if area.height < 2 {
        return false;
    }
    put_text(
        buffer,
        area.x,
        area.y + 1,
        area.width,
        " agents",
        Style::default()
            .fg(config.palette.overlay0)
            .add_modifier(Modifier::BOLD),
    );
    if let Some(label) = agent_view_label {
        let width = display_width(label).min(area.width as usize) as u16;
        put_text(
            buffer,
            area.right().saturating_sub(width),
            area.y + 1,
            width,
            label,
            Style::default()
                .fg(config.palette.accent)
                .add_modifier(Modifier::BOLD),
        );
    }
    true
}

pub(super) fn render_agent_list<T>(
    buffer: &mut Buffer,
    area: Rect,
    rows: &[T],
    empty_message: Option<&str>,
    config: &ClientShellConfig,
    agent_scroll: &mut usize,
    hits: &mut ShellHitMap,
    row_lines: impl Fn(&T) -> usize,
    mut render_row: impl FnMut(&mut Buffer, Rect, &T, &mut ShellHitMap),
) {
    let body = Rect::new(
        area.x,
        area.y.saturating_add(3),
        area.width,
        area.height.saturating_sub(3),
    );
    hits.agent_body = body;
    if body.is_empty() || rows.is_empty() {
        *agent_scroll = 0;
        if let Some(message) = empty_message.filter(|_| !body.is_empty()) {
            put_text(
                buffer,
                body.x,
                body.y,
                body.width,
                message,
                Style::default()
                    .fg(config.palette.overlay0)
                    .add_modifier(Modifier::DIM),
            );
        }
        return;
    }

    let row_heights = rows
        .iter()
        .map(|row| row_lines(row).max(1).min(u16::MAX as usize) as u16)
        .collect::<Vec<_>>();
    let gaps = rows
        .iter()
        .enumerate()
        .map(|(index, _)| {
            if index + 1 < rows.len() {
                config.agents.row_gap
            } else {
                0
            }
        })
        .collect::<Vec<_>>();
    let metrics =
        super::scroll::list_scroll_metrics(&row_heights, &gaps, body.height, *agent_scroll);
    hits.agent_max_scroll = metrics.max_offset_from_bottom;
    hits.agent_scroll_metrics = Some(metrics);
    *agent_scroll = metrics
        .max_offset_from_bottom
        .saturating_sub(metrics.offset_from_bottom);
    let show_scrollbar = metrics.max_offset_from_bottom > 0 && body.width > 1;
    let content_width = body.width.saturating_sub(u16::from(show_scrollbar));
    let mut y = body.y;
    for (index, row) in rows.iter().enumerate().skip(*agent_scroll) {
        let height = row_heights[index].min(body.height);
        if y.saturating_add(height) > body.bottom() {
            break;
        }
        let rect = Rect::new(body.x, y, content_width, height);
        render_row(buffer, rect, row, hits);
        y = y
            .saturating_add(height)
            .saturating_add(if index + 1 < rows.len() {
                config.agents.row_gap
            } else {
                0
            });
    }

    if show_scrollbar {
        let track = Rect::new(body.right().saturating_sub(1), body.y, 1, body.height);
        hits.agent_scrollbar = track;
        super::scroll::render_list_scrollbar(buffer, track, metrics, &config.palette);
    }
}

pub(super) fn agent_rows(
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    machine: Option<&str>,
) -> Vec<AgentRow> {
    agent_list_entries(snapshot, ordered_agents(snapshot))
        .into_iter()
        .filter_map(|(entry, depth)| agent_row(snapshot, entry, depth, config, machine))
        .collect()
}

pub(super) fn pane_label(pane: &ClientShellPane) -> &str {
    pane.tokens
        .iter()
        .find(|(key, _)| key == "p_name")
        .map(|(_, value)| value.as_str())
        .or(pane.title.as_deref())
        .or(pane.label.as_deref())
        .unwrap_or(&pane.pane_id)
}

pub(super) fn agent_row(
    snapshot: &ClientShellSnapshot,
    entry: AgentListEntry<'_>,
    depth: usize,
    config: &ClientShellConfig,
    machine: Option<&str>,
) -> Option<AgentRow> {
    let agent = match entry {
        AgentListEntry::Agent(agent) => agent,
        AgentListEntry::Pane(pane) => {
            return Some(AgentRow {
                pane_id: pane.pane_id.clone(),
                status: None,
                focused: pane.focused,
                depth,
                rows: vec![vec![crate::ui::ResolvedToken {
                    kind: crate::ui::ResolvedTokenKind::Pane(pane_label(pane).to_owned()),
                    style: Default::default(),
                }]],
            });
        }
    };
    let workspace = snapshot
        .workspaces
        .iter()
        .find(|workspace| workspace.workspace_id == agent.workspace_id)?;
    let tab = snapshot.tabs.iter().find(|tab| tab.tab_id == agent.tab_id);
    let pane = snapshot
        .panes
        .iter()
        .find(|pane| pane.pane_id == agent.pane_id);
    let tab_count = snapshot
        .tabs
        .iter()
        .filter(|candidate| candidate.workspace_id == agent.workspace_id)
        .count();
    let tab_label = tab
        .filter(|tab| tab_count > 1 || tab.custom_label)
        .map(|tab| tab.label.as_str());
    let agent_label = agent
        .display_agent
        .as_deref()
        .or(agent.name.as_deref())
        .or(agent.agent.as_deref())
        .or(agent.title.as_deref());
    let labels = agent
        .state_labels
        .iter()
        .cloned()
        .collect::<HashMap<_, _>>();
    let tokens = agent.tokens.iter().cloned().collect::<HashMap<_, _>>();
    let state_text = labels
        .get(status_text(agent.agent_status))
        .map(String::as_str)
        .unwrap_or_else(|| sidebar_status_text(agent.agent_status));
    let canonical_agent = agent
        .agent
        .as_deref()
        .and_then(crate::detect::parse_agent_label);
    let rows = crate::ui::sidebar_agent_rows(
        &config.agents,
        crate::ui::AgentTokenContext {
            machine,
            workspace: &workspace.label,
            tab: tab_label,
            pane: agent
                .title
                .as_deref()
                .or_else(|| pane.and_then(|pane| pane.label.as_deref())),
            agent_label,
            terminal_title: agent.terminal_title.as_deref(),
            terminal_title_stripped: agent.terminal_title_stripped.as_deref(),
            canonical_agent,
            tokens: &tokens,
        },
        state_text,
    );
    Some(AgentRow {
        pane_id: agent.pane_id.clone(),
        status: Some(agent.agent_status),
        focused: agent.focused,
        depth,
        rows,
    })
}

pub(super) fn render_agent_row(
    buffer: &mut Buffer,
    rect: Rect,
    row: &AgentRow,
    config: &ClientShellConfig,
) {
    let palette = &config.palette;
    let row_style = if row.focused {
        Style::default().bg(palette.active_row_bg)
    } else {
        Style::default()
    };
    let name_style = if row.focused {
        Style::default()
            .fg(palette.text)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(palette.subtext0)
            .add_modifier(Modifier::BOLD)
    };
    let status_style = Style::default().fg(row
        .status
        .map_or(palette.overlay0, |status| status_color(status, palette)));
    let secondary = Style::default().fg(palette.overlay0);
    let icon = (
        row.status
            .map_or("", |status| status_icon(status, config.status_indicators)),
        status_style,
    );
    let rows = if row.rows.is_empty() {
        vec![vec![crate::ui::ResolvedToken {
            kind: crate::ui::ResolvedTokenKind::StateIcon,
            style: Default::default(),
        }]]
    } else {
        row.rows.clone()
    };
    for (index, tokens) in rows.iter().take(rect.height as usize).enumerate() {
        let indent = if index == 0 {
            1 + row.depth
        } else {
            3 + row.depth
        };
        let mut spans = if index == 0 && row.depth > 0 {
            vec![
                ratatui::text::Span::raw(" ".repeat(row.depth)),
                ratatui::text::Span::styled("└", secondary),
            ]
        } else {
            vec![ratatui::text::Span::raw(" ".repeat(indent))]
        };
        spans.extend(crate::ui::resolved_token_spans(
            tokens,
            icon,
            status_style,
            name_style,
            secondary,
            secondary,
            palette,
            (rect.width as usize).saturating_sub(indent),
        ));
        Paragraph::new(Line::from(spans)).style(row_style).render(
            Rect::new(rect.x, rect.y + index as u16, rect.width, 1),
            buffer,
        );
    }
}

fn put_text(buffer: &mut Buffer, x: u16, y: u16, width: u16, text: &str, style: Style) {
    for (offset, character) in text.chars().take(width as usize).enumerate() {
        if let Some(cell) = buffer.cell_mut((x + offset as u16, y)) {
            cell.set_char(character).set_style(style);
        }
    }
}

fn display_width(text: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(text)
}

fn sidebar_status_text(status: crate::api::schema::AgentStatus) -> &'static str {
    use crate::api::schema::AgentStatus;
    match status {
        AgentStatus::Blocked => "blocked",
        AgentStatus::Done => "done",
        AgentStatus::Working => "working",
        AgentStatus::Idle | AgentStatus::Unknown => "idle",
    }
}
