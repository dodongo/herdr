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
