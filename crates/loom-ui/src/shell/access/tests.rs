use super::*;
use accesskit::{Node, NodeId, Role, TreeId, TreeInfo};

/// A whole tree of one window node, named `name`.
fn tree(name: &str) -> TreeUpdate {
    let mut window = Node::new(Role::Window);
    window.set_label(name);
    TreeUpdate {
        nodes: vec![(NodeId(1), window)],
        tree: Some(TreeInfo::new(NodeId(1))),
        tree_id: TreeId::ROOT,
        focus: NodeId(1),
    }
}

/// The label of a tree's first node, to tell trees apart.
fn label(update: &TreeUpdate) -> Option<String> {
    update
        .nodes
        .first()
        .and_then(|(_, node)| node.label().map(str::to_string))
}

/// A screen reader that starts before the app has built a tree gets none,
/// and the next update hands it the whole tree, not only the changes; the
/// one after hands it the changes.
#[test]
fn a_screen_reader_before_the_first_tree_gets_the_whole_tree_next() {
    let shared = SharedTree::default();
    assert!(shared.activate().is_none(), "nothing to hand over yet");
    let first = shared.hand_over(tree("whole"), Some(tree("changes")));
    assert_eq!(first.as_ref().and_then(label).as_deref(), Some("whole"));
    let second = shared.hand_over(tree("whole 2"), Some(tree("changes 2")));
    assert_eq!(second.as_ref().and_then(label).as_deref(), Some("changes 2"));
}

/// A screen reader that starts after a tree was given gets the last whole
/// tree at once, and then only changes.
#[test]
fn a_screen_reader_after_a_tree_gets_it_at_once() {
    let shared = SharedTree::default();
    assert!(
        shared.hand_over(tree("before"), None).is_none(),
        "no reader, nothing to apply"
    );
    let at_once = shared.activate();
    assert_eq!(at_once.as_ref().and_then(label).as_deref(), Some("before"));
    let next = shared.hand_over(tree("whole"), Some(tree("changes")));
    assert_eq!(next.as_ref().and_then(label).as_deref(), Some("changes"));
}

/// The adapter's events become the runtime's.
#[test]
fn the_adapters_events_become_the_runtimes() {
    use accesskit::{Action, ActionRequest};
    let request = ActionRequest {
        action: Action::Click,
        target_tree: TreeId::ROOT,
        target_node: NodeId(3),
        data: None,
    };
    assert_eq!(
        event(accesskit_winit::WindowEvent::ActionRequested(request.clone())),
        AccessEvent::Action(request)
    );
    assert_eq!(
        event(accesskit_winit::WindowEvent::AccessibilityDeactivated),
        AccessEvent::Deactivated
    );
    assert_eq!(
        event(accesskit_winit::WindowEvent::InitialTreeRequested),
        AccessEvent::Activated
    );
}
