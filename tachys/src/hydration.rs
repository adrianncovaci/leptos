use crate::{
    renderer::{CastFrom, Rndr},
    view::{Position, PositionState},
};
#[cfg(any(debug_assertions, leptos_debuginfo))]
use std::cell::Cell;
use std::{cell::RefCell, panic::Location, rc::Rc};
#[cfg(any(debug_assertions, leptos_debuginfo))]
use wasm_bindgen::JsCast;
use web_sys::{Comment, Element, Node, Text};

#[cfg(feature = "mark_branches")]
const COMMENT_NODE: u16 = 8;

#[cfg(feature = "mark_branches")]
#[derive(Debug)]
pub(crate) struct BranchMarker {
    node: crate::renderer::types::Node,
    range_start: crate::renderer::types::Node,
    id: String,
}

#[cfg(feature = "mark_branches")]
impl BranchMarker {
    pub(crate) fn id(&self) -> &str {
        &self.id
    }
}

#[cfg(feature = "mark_branches")]
fn branch_marker(
    node: &crate::renderer::types::Node,
) -> Option<(bool, String)> {
    if node.node_type() != COMMENT_NODE {
        return None;
    }

    let content = node.text_content()?;
    if let Some(id) = content.strip_prefix("bo-") {
        Some((true, id.to_string()))
    } else {
        content
            .strip_prefix("bc-")
            .map(|id| (false, id.to_string()))
    }
}

/// Hydration works by walking over the DOM, adding interactivity as needed.
///
/// This cursor tracks the location in the DOM that is currently being hydrated. Each that type
/// implements [`RenderHtml`](crate::view::RenderHtml) knows how to advance the cursor to access
/// the nodes it needs.
#[derive(Debug)]
pub struct Cursor(Rc<RefCell<crate::renderer::types::Node>>);

impl Clone for Cursor {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

impl Cursor
where
    crate::renderer::types::Element: AsRef<crate::renderer::types::Node>,
{
    /// Creates a new cursor starting at the root element.
    pub fn new(root: crate::renderer::types::Element) -> Self {
        let root = <crate::renderer::types::Element as AsRef<
            crate::renderer::types::Node,
        >>::as_ref(&root)
        .clone();
        Self(Rc::new(RefCell::new(root)))
    }

    /// Returns the node at which the cursor is currently located.
    pub fn current(&self) -> crate::renderer::types::Node {
        self.0.borrow().clone()
    }

    #[cfg(feature = "mark_branches")]
    pub(crate) fn next_branch_marker(
        &self,
        position: &PositionState,
    ) -> Option<BranchMarker> {
        let current = self.current();
        let node = match position.get() {
            Position::Current => Some(current),
            Position::FirstChild => Rndr::first_child(&current),
            _ => Rndr::next_sibling(&current),
        }?;

        let (is_opening, id) = branch_marker(&node)?;
        is_opening.then(|| BranchMarker {
            range_start: node.clone(),
            node,
            id,
        })
    }

    #[cfg(feature = "mark_branches")]
    pub(crate) fn next_branch_marker_matching(
        &self,
        position: &PositionState,
        candidates: &[&str],
    ) -> Option<BranchMarker> {
        let first = self.next_branch_marker(position)?;
        let mut node = Some(first.node);

        while let Some(current) = node {
            let (is_opening, id) = branch_marker(&current)?;
            if !is_opening {
                return None;
            }
            if candidates.iter().any(|candidate| *candidate == id) {
                return Some(BranchMarker {
                    range_start: current.clone(),
                    node: current,
                    id,
                });
            }
            node = Rndr::next_sibling(&current);
        }

        None
    }

    #[cfg(feature = "mark_branches")]
    pub(crate) fn replace_next_branch(
        &self,
        position: &PositionState,
        candidates: &[&str],
        replacement: &mut dyn crate::view::Mountable,
    ) {
        let Some(opening) =
            self.next_branch_marker_matching(position, candidates)
        else {
            return;
        };
        let Some(parent) = opening.range_start.parent_element() else {
            return;
        };

        let mut depth = 0usize;
        let mut current = Some(opening.range_start);
        let mut after = None;

        while let Some(node) = current {
            let next = node.next_sibling();
            if let Some((is_opening, _)) = branch_marker(&node) {
                if is_opening {
                    depth += 1;
                } else {
                    depth = depth.saturating_sub(1);
                }
            }

            Rndr::remove_node(&parent, &node);

            if depth == 0 {
                after = next;
                break;
            }
            current = next;
        }

        replacement.mount(&parent, after.as_ref());
        let current =
            after.as_ref().and_then(Node::previous_sibling).or_else(|| {
                <crate::renderer::types::Element as AsRef<
                    crate::renderer::types::Node,
                >>::as_ref(&parent)
                .last_child()
            });
        if let Some(current) = current {
            self.set(current);
        }
        position.set(Position::NextChild);
    }

    /// Advances to the next child of the node at which the cursor is located.
    ///
    /// Does nothing if there is no child.
    pub fn child(&self) {
        let mut inner = self.0.borrow_mut();
        if let Some(node) = Rndr::first_child(&inner) {
            *inner = node;
        }

        #[cfg(feature = "mark_branches")]
        {
            while inner.node_type() == COMMENT_NODE {
                if let Some(content) = inner.text_content() {
                    if content.starts_with("bo") || content.starts_with("bc") {
                        if let Some(sibling) = Rndr::next_sibling(&inner) {
                            *inner = sibling;
                            continue;
                        }
                    }
                }

                break;
            }
        }
        // //drop(inner);
        //crate::log(">> which is ");
        //Rndr::log_node(&self.current());
    }

    /// Advances to the next sibling of the node at which the cursor is located.
    ///
    /// Does nothing if there is no sibling.
    pub fn sibling(&self) {
        let mut inner = self.0.borrow_mut();
        if let Some(node) = Rndr::next_sibling(&inner) {
            *inner = node;
        }

        #[cfg(feature = "mark_branches")]
        {
            while inner.node_type() == COMMENT_NODE {
                if let Some(content) = inner.text_content() {
                    if content.starts_with("bo") || content.starts_with("bc") {
                        if let Some(sibling) = Rndr::next_sibling(&inner) {
                            *inner = sibling;
                            continue;
                        }
                    }
                }
                break;
            }
        }
        //drop(inner);
        //crate::log(">> which is ");
        //Rndr::log_node(&self.current());
    }

    /// Moves to the parent of the node at which the cursor is located.
    ///
    /// Does nothing if there is no parent.
    pub fn parent(&self) {
        let mut inner = self.0.borrow_mut();
        if let Some(node) = Rndr::get_parent(&inner) {
            *inner = node;
        }
    }

    /// Sets the cursor to some node.
    pub fn set(&self, node: crate::renderer::types::Node) {
        *self.0.borrow_mut() = node;
    }

    /// Advances to the next placeholder node and returns it
    pub fn next_placeholder(
        &self,
        position: &PositionState,
    ) -> crate::renderer::types::Placeholder {
        //crate::dom::log("looking for placeholder after");
        //Rndr::log_node(&self.current());
        self.advance_to_placeholder(position);
        let marker = self.current();
        crate::renderer::types::Placeholder::cast_from(marker.clone())
            .unwrap_or_else(|| failed_to_cast_marker_node(marker))
    }

    /// Advances to the next placeholder node.
    pub fn advance_to_placeholder(&self, position: &PositionState) {
        if position.get() == Position::FirstChild {
            self.child();
        } else {
            self.sibling();
        }
        position.set(Position::NextChild);
    }
}

#[cfg(any(debug_assertions, leptos_debuginfo))]
thread_local! {
    static CURRENTLY_HYDRATING: Cell<Option<&'static Location<'static>>> = const { Cell::new(None) };
}

pub(crate) fn set_currently_hydrating(
    location: Option<&'static Location<'static>>,
) {
    #[cfg(any(debug_assertions, leptos_debuginfo))]
    {
        CURRENTLY_HYDRATING.set(location);
    }
    #[cfg(not(any(debug_assertions, leptos_debuginfo)))]
    {
        _ = location;
    }
}

pub(crate) fn failed_to_cast_element(tag_name: &str, node: Node) -> Element {
    #[cfg(not(any(debug_assertions, leptos_debuginfo)))]
    {
        _ = node;
        unreachable!();
    }
    #[cfg(any(debug_assertions, leptos_debuginfo))]
    {
        let hydrating = CURRENTLY_HYDRATING
            .take()
            .map(|n| n.to_string())
            .unwrap_or_else(|| "{unknown}".to_string());
        // [hydra-trace] Maximum panic context dump
        let node_type = node.node_type();
        let node_name = node.node_name();
        let node_value = node.node_value().unwrap_or_default();
        let outer = node
            .clone()
            .dyn_into::<web_sys::Element>()
            .ok()
            .and_then(|e| Some(e.outer_html()))
            .unwrap_or_else(|| "<not-an-element>".into());
        let parent_outer = node
            .parent_element()
            .map(|p| p.outer_html())
            .unwrap_or_else(|| "<no-parent>".into());
        let prev_sibling = node
            .previous_sibling()
            .map(|s| {
                format!(
                    "type={} name={} value={:?}",
                    s.node_type(),
                    s.node_name(),
                    s.node_value()
                )
            })
            .unwrap_or_else(|| "<none>".into());
        let next_sibling = node
            .next_sibling()
            .map(|s| {
                format!(
                    "type={} name={} value={:?}",
                    s.node_type(),
                    s.node_name(),
                    s.node_value()
                )
            })
            .unwrap_or_else(|| "<none>".into());
        web_sys::console::warn_1(
            &format!(
                "[hydra-trace] PANIC failed_to_cast_element\n  \
                 expected_tag=<{tag_name}>\n  defined_at={hydrating}\n  \
                 found_node_type={node_type}\n  found_node_name={node_name}\n  \
                 found_node_value={node_value:?}\n  found_outer={outer}\n  \
                 prev_sibling={prev_sibling}\n  next_sibling={next_sibling}\n  \
                 parent_outer={}",
                if parent_outer.len() > 500 {
                    format!(
                        "{}…[truncated, total {}b]",
                        &parent_outer[..500],
                        parent_outer.len()
                    )
                } else {
                    parent_outer
                },
            )
            .into(),
        );
        web_sys::console::error_3(
            &wasm_bindgen::JsValue::from_str(&format!(
                "A hydration error occurred while trying to hydrate an \
                 element defined at {hydrating}.\n\nThe framework expected an \
                 HTML <{tag_name}> element, but found this instead: ",
            )),
            &node,
            &wasm_bindgen::JsValue::from_str(
                "\n\nThe hydration mismatch may have occurred slightly \
                 earlier, but this is the first time the framework found a \
                 node of an unexpected type.",
            ),
        );
        panic!(
            "Unrecoverable hydration error. Please read the error message \
             directly above this for more details."
        );
    }
}

pub(crate) fn failed_to_cast_marker_node(node: Node) -> Comment {
    #[cfg(not(any(debug_assertions, leptos_debuginfo)))]
    {
        _ = node;
        unreachable!();
    }
    #[cfg(any(debug_assertions, leptos_debuginfo))]
    {
        let hydrating = CURRENTLY_HYDRATING
            .take()
            .map(|n| n.to_string())
            .unwrap_or_else(|| "{unknown}".to_string());
        web_sys::console::error_3(
            &wasm_bindgen::JsValue::from_str(&format!(
                "A hydration error occurred while trying to hydrate an \
                 element defined at {hydrating}.\n\nThe framework expected a \
                 marker node, but found this instead: ",
            )),
            &node,
            &wasm_bindgen::JsValue::from_str(
                "\n\nThe hydration mismatch may have occurred slightly \
                 earlier, but this is the first time the framework found a \
                 node of an unexpected type.",
            ),
        );
        panic!(
            "Unrecoverable hydration error. Please read the error message \
             directly above this for more details."
        );
    }
}

pub(crate) fn failed_to_cast_text_node(node: Node) -> Text {
    #[cfg(not(any(debug_assertions, leptos_debuginfo)))]
    {
        _ = node;
        unreachable!();
    }
    #[cfg(any(debug_assertions, leptos_debuginfo))]
    {
        let hydrating = CURRENTLY_HYDRATING
            .take()
            .map(|n| n.to_string())
            .unwrap_or_else(|| "{unknown}".to_string());
        web_sys::console::error_3(
            &wasm_bindgen::JsValue::from_str(&format!(
                "A hydration error occurred while trying to hydrate an \
                 element defined at {hydrating}.\n\nThe framework expected a \
                 text node, but found this instead: ",
            )),
            &node,
            &wasm_bindgen::JsValue::from_str(
                "\n\nThe hydration mismatch may have occurred slightly \
                 earlier, but this is the first time the framework found a \
                 node of an unexpected type.",
            ),
        );
        panic!(
            "Unrecoverable hydration error. Please read the error message \
             directly above this for more details."
        );
    }
}
