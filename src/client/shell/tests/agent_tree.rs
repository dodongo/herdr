use super::super::agent_sidebar::{
    agent_rows, ordered_agent_pane_ids, ordered_agents, render_agent_row,
};
use super::*;

fn agent(pane_id: &str, parent: Option<&str>) -> ClientShellAgent {
    ClientShellAgent {
        pane_id: pane_id.into(),
        workspace_id: "ws_1".into(),
        tab_id: "tab_1".into(),
        name: Some(pane_id.into()),
        display_agent: None,
        agent: Some("pi".into()),
        title: None,
        terminal_title: None,
        terminal_title_stripped: None,
        agent_status: AgentStatus::Working,
        state_change_seq: 1,
        state_labels: Vec::new(),
        tokens: parent
            .map(|parent| vec![("p_parent_pane".into(), parent.into())])
            .unwrap_or_default(),
        focused: false,
    }
}

fn plain_pane(pane_id: &str, parent: &str) -> ClientShellPane {
    ClientShellPane {
        pane_id: pane_id.into(),
        title: Some(format!("{pane_id} title")),
        focused: false,
        tokens: vec![
            ("p_presentation".into(), "pane".into()),
            ("p_parent_pane".into(), parent.into()),
        ],
        ..snapshot().panes.remove(0)
    }
}

#[test]
fn plain_panes_follow_agent_children_at_their_parents_depth() {
    let mut projected = snapshot();
    projected.agents = vec![
        agent("parent", None),
        agent("child", Some("parent")),
        agent("grandchild", Some("child")),
        agent("sibling", Some("parent")),
        agent("other", None),
    ];
    projected.panes = vec![
        plain_pane("parent-shell-1", "parent"),
        plain_pane("child-shell", "child"),
        plain_pane("parent-shell-2", "parent"),
        plain_pane("other-shell", "other"),
    ];
    let config = ClientShellConfig::from_config(&Config::default());
    let rows = agent_rows(&projected, &config, None);
    assert_eq!(
        rows.iter()
            .map(|row| (row.pane_id.as_str(), row.depth))
            .collect::<Vec<_>>(),
        [
            ("parent", 0),
            ("child", 1),
            ("grandchild", 2),
            ("child-shell", 2),
            ("sibling", 1),
            ("parent-shell-1", 1),
            ("parent-shell-2", 1),
            ("other", 0),
            ("other-shell", 1),
        ],
    );
    assert_eq!(
        ordered_agent_pane_ids(&projected),
        rows.iter()
            .map(|row| row.pane_id.clone())
            .collect::<Vec<_>>(),
    );
}

#[test]
fn plain_pane_label_uses_name_then_title_and_never_renders_agent_state() {
    let mut projected = snapshot();
    projected.agents = vec![agent("parent", None)];
    let mut pane = plain_pane("shell", "parent");
    pane.tokens.push(("p_name".into(), "build output".into()));
    projected.panes = vec![pane];
    let config = ClientShellConfig::from_config(&Config::default());
    for expected in ["build output", "shell title"] {
        let rows = agent_rows(&projected, &config, None);
        let row = &rows[1];
        assert_eq!(row.status, None);
        assert_eq!(row.rows.len(), 1);
        assert!(matches!(
            row.rows[0].as_slice(),
            [crate::ui::ResolvedToken { kind: crate::ui::ResolvedTokenKind::Pane(label), .. }]
                if label == expected
        ));
        let area = Rect::new(0, 0, 40, 1);
        let mut buffer = Buffer::empty(area);
        render_agent_row(&mut buffer, area, row, &config);
        let line = (0..area.width)
            .map(|x| buffer[(x, 0)].symbol())
            .collect::<String>();
        assert_eq!(line.trim_end(), format!(" └{expected}"));
        projected.panes[0].tokens.retain(|(key, _)| key != "p_name");
    }
}

#[test]
fn plain_panes_require_both_tokens_and_a_visible_parent_and_disappear_on_close() {
    let mut projected = snapshot();
    projected.agents = vec![agent("parent", None)];
    let mut untagged = plain_pane("untagged", "parent");
    untagged.tokens.clear();
    let mut no_parent = plain_pane("no-parent", "parent");
    no_parent.tokens.retain(|(key, _)| key != "p_parent_pane");
    let mut no_presentation = plain_pane("no-presentation", "parent");
    no_presentation
        .tokens
        .retain(|(key, _)| key != "p_presentation");
    let mut subagent = plain_pane("not-a-plain-pane", "parent");
    subagent.tokens[0].1 = "subagent".into();
    projected.panes = vec![
        untagged,
        no_parent,
        no_presentation,
        subagent,
        plain_pane("orphan", "missing"),
        plain_pane("shell", "parent"),
        plain_pane("parent", "parent"),
    ];
    assert_eq!(ordered_agent_pane_ids(&projected), ["parent", "shell"]);
    projected.panes.retain(|pane| pane.pane_id != "shell");
    assert_eq!(ordered_agent_pane_ids(&projected), ["parent"]);
    projected.panes.push(plain_pane("shell", "parent"));
    projected.agents.clear();
    assert!(ordered_agent_pane_ids(&projected).is_empty());
}

#[test]
fn plain_panes_follow_only_parents_in_the_active_agent_view() {
    let mut projected = snapshot();
    projected.agents = vec![agent("parent", None), agent("other", None)];
    projected.panes = vec![
        plain_pane("shell", "parent"),
        plain_pane("hidden-shell", "other"),
    ];
    projected.agent_view_label = Some("filtered".into());
    projected.agent_order = vec!["parent".into()];
    assert_eq!(ordered_agent_pane_ids(&projected), ["parent", "shell"]);
    let config = ClientShellConfig::from_config(&Config::default());
    assert_eq!(agent_rows(&projected, &config, None)[1].depth, 1);
}

#[test]
fn plain_pane_navigation_and_click_focus_the_pane_without_changing_agent_counts() {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};

    let mut projected = snapshot();
    projected.agents = vec![agent("pane_1", None)];
    projected.panes.push(plain_pane("shell", "pane_1"));
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(projected));
    state.set_pane_surface(surface());
    state.compose(106, 30).expect("pane sidebar");
    assert_eq!(state.snapshot.as_ref().unwrap().agents.len(), 1);
    let hit = state
        .hits
        .agents
        .iter()
        .find(|(_, id)| id == "shell")
        .unwrap()
        .0;
    let clicked = state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: hit.x,
        row: hit.y,
        modifiers: KeyModifiers::empty(),
    })]);
    assert!(clicked.actions.iter().any(|action| matches!(
        action,
        ClientShellAction::Endpoint { request, .. }
            if matches!(&request.method, crate::api::schema::Method::PaneFocus(target) if target.pane_id == "shell")
    )));
    let mut outcome = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::NextAgent),
        &mut outcome,
    );
    assert!(outcome.actions.iter().any(|action| matches!(
        action,
        ClientShellAction::Endpoint { request, .. }
            if matches!(&request.method, crate::api::schema::Method::PaneFocus(target) if target.pane_id == "shell")
    )));
}

#[test]
fn plain_panes_remain_focusable_in_collapsed_and_mobile_lists() {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};

    let mut projected = snapshot();
    projected.agents = vec![agent("pane_1", None)];
    projected.panes.push(plain_pane("shell", "pane_1"));
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(projected));
    state.set_pane_surface(surface());
    state.sidebar_collapsed = true;
    state.compose(106, 30).unwrap();
    assert_eq!(
        state
            .hits
            .agents
            .iter()
            .map(|(_, pane)| pane.as_str())
            .collect::<Vec<_>>(),
        ["pane_1", "shell"],
    );
    state.mode = ClientShellMode::Navigate;
    let frame = state.compose(44, 30).unwrap();
    let text = frame
        .cells
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect::<String>();
    assert!(text.contains("└shell title"));
    let hit = state
        .hits
        .mobile_targets
        .iter()
        .find_map(|(rect, target)| {
            matches!(target, ClientMobileTarget::Agent { pane_id, .. } if pane_id == "shell")
                .then_some(*rect)
        })
        .unwrap();
    let focused = state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: hit.x,
        row: hit.y,
        modifiers: KeyModifiers::empty(),
    })]);
    assert!(focused.actions.iter().any(|action| matches!(
        action,
        ClientShellAction::Endpoint { request, .. }
            if matches!(&request.method, crate::api::schema::Method::PaneFocus(target) if target.pane_id == "shell")
    )));
}

#[test]
fn agent_tree_preserves_root_and_sibling_order_with_nested_and_missing_parents() {
    let mut projected = snapshot();
    projected.agents = vec![
        agent("w2:p11D", Some("w2:p11C")),
        agent("w1:p1", None),
        agent("w2:p11C", Some("w2:p11B")),
        agent("w3:p1", Some("w9:p9")),
        agent("w2:p11B", None),
        agent("w2:p11E", Some("w2:p11B")),
    ];

    assert_eq!(
        ordered_agents(&projected)
            .into_iter()
            .map(|(agent, depth)| (agent.pane_id.as_str(), depth))
            .collect::<Vec<_>>(),
        vec![
            ("w1:p1", 0),
            ("w3:p1", 0),
            ("w2:p11B", 0),
            ("w2:p11C", 1),
            ("w2:p11D", 2),
            ("w2:p11E", 1),
        ]
    );
    assert_eq!(
        ordered_agent_pane_ids(&projected),
        ["w1:p1", "w3:p1", "w2:p11B", "w2:p11C", "w2:p11D", "w2:p11E"]
    );
}

#[test]
fn agent_tree_treats_cycle_members_and_self_parents_as_roots() {
    let mut projected = snapshot();
    projected.agents = vec![
        agent("child", Some("cycle-b")),
        agent("cycle-a", Some("cycle-b")),
        agent("cycle-b", Some("cycle-a")),
        agent("self", Some("self")),
    ];
    assert_eq!(
        ordered_agents(&projected)
            .into_iter()
            .map(|(agent, depth)| (agent.pane_id.as_str(), depth))
            .collect::<Vec<_>>(),
        vec![("cycle-a", 0), ("cycle-b", 0), ("child", 1), ("self", 0)]
    );
}

#[test]
fn agent_view_keeps_server_order_without_tree_indentation() {
    let mut projected = snapshot();
    projected.agents = vec![agent("parent", None), agent("child", Some("parent"))];
    projected.agent_view_label = Some("review".into());
    projected.agent_order = vec!["child".into(), "missing".into(), "parent".into()];
    assert_eq!(
        ordered_agents(&projected)
            .into_iter()
            .map(|(agent, depth)| (agent.pane_id.as_str(), depth))
            .collect::<Vec<_>>(),
        vec![("child", 0), ("parent", 0)]
    );
}

#[test]
fn agent_navigation_uses_the_sidebar_tree_order() {
    let mut projected = snapshot();
    projected.agents = vec![
        agent("grandchild", Some("child")),
        agent("pane_1", None),
        agent("other", None),
        agent("child", Some("pane_1")),
    ];
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(projected));
    state.set_pane_surface(surface());
    state.compose(106, 30).expect("tree sidebar");
    assert_eq!(
        state
            .hits
            .agents
            .iter()
            .map(|(_, id)| id.as_str())
            .collect::<Vec<_>>(),
        ["pane_1", "child", "grandchild", "other"]
    );
    for (action, expected) in [
        (crate::input::KeybindAction::FocusAgent(2), "grandchild"),
        (crate::input::KeybindAction::NextAgent, "child"),
        (crate::input::KeybindAction::PreviousAgent, "other"),
    ] {
        let mut outcome = ClientShellInput::default();
        state.record_binding(crate::input::KeybindMatch::Action(action), &mut outcome);
        assert!(matches!(
            outcome.actions.as_slice(),
            [ClientShellAction::Endpoint { request, .. }]
                if matches!(&request.method, crate::api::schema::Method::PaneFocus(target)
                    if target.pane_id == expected)
        ));
    }
}

#[test]
fn agent_tree_indents_first_and_later_lines_one_column_per_level() {
    use crate::config::AgentSidebarToken;

    let mut projected = snapshot();
    projected.agents = vec![
        agent("parent", None),
        agent("child", Some("parent")),
        agent("grandchild", Some("child")),
    ];
    let mut config = Config::default();
    config.ui.sidebar.agents.rows = vec![
        vec![AgentSidebarToken::StateIcon, AgentSidebarToken::Agent],
        vec![AgentSidebarToken::Agent],
    ];
    let config = ClientShellConfig::from_config(&config);
    let rows = agent_rows(&projected, &config, None);
    for (row, prefix, later_prefix) in [
        (&rows[0], " ●", "   parent"),
        (&rows[1], " └●", "    child"),
        (&rows[2], "  └●", "     grandchild"),
    ] {
        let area = Rect::new(2, 1, 30, 2);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 34, 4));
        render_agent_row(&mut buffer, area, row, &config);
        let line = |y| {
            (area.x..area.right())
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        };
        assert!(line(area.y).starts_with(prefix), "{}", line(area.y));
        assert!(
            line(area.y + 1).starts_with(later_prefix),
            "{}",
            line(area.y + 1)
        );
        if row.depth > 0 {
            assert_eq!(
                buffer[(area.x + row.depth as u16, area.y)].fg,
                config.palette.overlay0
            );
        }
    }
}
