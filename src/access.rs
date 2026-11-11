//! What Settings and History tell assistive technology about themselves.
//!
//! Both windows are drawn by screen-mark, pixel by pixel, so the system has
//! no idea what is in them. Each can describe itself as a `Snapshot`: its
//! controls in reading order, what each is called, its state, where it is.
//! On Windows that description is handed to UI Automation (through
//! AccessKit), which is what Narrator, NVDA and JAWS read; see
//! `access::Bridge`. The same list drives keyboard focus on every platform.

/// What kind of control something is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Tab,
    Button,
    CheckBox,
    /// One of a set where exactly one is chosen.
    Radio,
}

/// One control, as a screen reader should present it.
#[derive(Debug, Clone, PartialEq)]
pub struct Control {
    pub role: Role,
    /// What it is called.
    pub label: String,
    /// On or off, for a check box; chosen or not, for a tab or radio button.
    pub checked: Option<bool>,
    /// What it currently holds (a key button's binding).
    pub value: Option<String>,
    /// Where it is in the window, in logical pixels: (x, y, w, h). `None`
    /// while it is scrolled out of view.
    pub rect: Option<(f32, f32, f32, f32)>,
}

/// A window's controls at one moment.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Snapshot {
    /// The window's name.
    pub title: String,
    /// Every control, in reading order.
    pub controls: Vec<Control>,
    /// The index in `controls` of the one with keyboard focus.
    pub focus: Option<usize>,
    /// A message the window is showing ("Ctrl+P is taken by another app"),
    /// read out when it changes.
    pub status: Option<String>,
}

/// What assistive technology asked a window to do to one of its controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// Press it: what Enter or a click would do.
    Press(usize),
    /// Move keyboard focus to it.
    Focus(usize),
}

#[cfg(target_os = "windows")]
pub use windows_bridge::Bridge;

/// Other platforms: nothing is told to assistive technology yet. The
/// keyboard still works; a screen reader finds an unlabelled window.
#[cfg(not(target_os = "windows"))]
pub struct Bridge;

#[cfg(not(target_os = "windows"))]
impl Bridge {
    pub fn new(
        _event_loop: &winit::event_loop::ActiveEventLoop,
        _window: &winit::window::Window,
        _wake: impl Fn() + Send + 'static,
    ) -> Self {
        Bridge
    }
    pub fn update(&mut self, _snapshot: Snapshot, _scale: f64) {}
    pub fn requests(&mut self) -> Vec<Request> {
        Vec::new()
    }
}

#[cfg(target_os = "windows")]
mod windows_bridge {
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::sync::{Arc, Mutex};

    use accesskit::{
        Action, ActionHandler, ActionRequest, ActivationHandler, Affine, DeactivationHandler, Live,
        Node, NodeId, Rect, Role as NodeRole, Toggled, TreeId, TreeInfo, TreeUpdate,
    };
    use winit::event_loop::ActiveEventLoop;
    use winit::window::Window;

    use super::{Request, Role, Snapshot};

    const ROOT: NodeId = NodeId(0);
    const STATUS: NodeId = NodeId(1);
    /// Control `i` of a snapshot is node `FIRST_CONTROL + i`.
    const FIRST_CONTROL: u64 = 16;

    /// A snapshot and the window's scale factor (controls are in logical
    /// pixels; UI Automation wants physical ones).
    type Described = (Snapshot, f64);

    /// One window's link to UI Automation. Keep it as long as the window,
    /// and drop it first.
    pub struct Bridge {
        adapter: accesskit_winit::Adapter,
        /// What was last described, for a screen reader that starts later.
        latest: Arc<Mutex<Described>>,
        requests: Receiver<Request>,
    }

    impl Bridge {
        /// `window` must not have been shown yet (create it with
        /// `with_visible(false)`, make the bridge, then show it). `wake` is
        /// called when a request arrives, so the event loop can pick it up
        /// with `requests`.
        pub fn new(
            event_loop: &ActiveEventLoop,
            window: &Window,
            wake: impl Fn() + Send + 'static,
        ) -> Self {
            let latest = Arc::new(Mutex::new((Snapshot::default(), 1.0)));
            let (tx, requests) = channel();
            let adapter = accesskit_winit::Adapter::with_direct_handlers(
                event_loop,
                window,
                Activation(Arc::clone(&latest)),
                Actions {
                    tx,
                    wake: Box::new(wake),
                },
                Deactivation,
            );
            Self {
                adapter,
                latest,
                requests,
            }
        }

        /// Describe the window as it is now. Cheap when nothing changed, and
        /// when no assistive technology is listening.
        pub fn update(&mut self, snapshot: Snapshot, scale: f64) {
            {
                let mut latest = self.latest.lock().unwrap_or_else(|p| p.into_inner());
                if latest.0 == snapshot && latest.1 == scale {
                    return;
                }
                *latest = (snapshot.clone(), scale);
            }
            self.adapter.update_if_active(|| tree(&snapshot, scale));
        }

        /// What assistive technology asked for since the last call.
        pub fn requests(&mut self) -> Vec<Request> {
            self.requests.try_iter().collect()
        }
    }

    /// The whole window as an accessibility tree: the window, its controls
    /// in reading order, and the status message if there is one.
    pub(super) fn tree(snapshot: &Snapshot, scale: f64) -> TreeUpdate {
        let mut nodes = Vec::with_capacity(snapshot.controls.len() + 2);
        let mut children = Vec::with_capacity(snapshot.controls.len() + 1);
        for (i, control) in snapshot.controls.iter().enumerate() {
            let id = NodeId(FIRST_CONTROL + i as u64);
            let mut node = Node::new(match control.role {
                Role::Tab => NodeRole::Tab,
                Role::Button => NodeRole::Button,
                Role::CheckBox => NodeRole::CheckBox,
                Role::Radio => NodeRole::RadioButton,
            });
            node.set_label(control.label.as_str());
            if let Some(value) = &control.value {
                node.set_value(value.as_str());
            }
            match (control.role, control.checked) {
                (Role::Tab, Some(on)) => node.set_selected(on),
                (_, Some(on)) => node.set_toggled(Toggled::from(on)),
                (_, None) => {}
            }
            // A control scrolled out of view stays in the tree, without a
            // place: focusing it scrolls it in.
            if let Some((x, y, w, h)) = control.rect {
                node.set_bounds(Rect {
                    x0: x as f64,
                    y0: y as f64,
                    x1: (x + w) as f64,
                    y1: (y + h) as f64,
                });
            }
            node.add_action(Action::Click);
            node.add_action(Action::Focus);
            children.push(id);
            nodes.push((id, node));
        }
        if let Some(status) = &snapshot.status {
            // Read out when it changes, without taking focus.
            let mut node = Node::new(NodeRole::Label);
            node.set_value(status.as_str());
            node.set_live(Live::Polite);
            children.push(STATUS);
            nodes.push((STATUS, node));
        }
        let mut root = Node::new(NodeRole::Window);
        root.set_label(snapshot.title.as_str());
        root.set_transform(Affine::scale(scale));
        root.set_children(children);
        nodes.push((ROOT, root));
        TreeUpdate {
            nodes,
            tree: Some(TreeInfo::new(ROOT)),
            tree_id: TreeId::ROOT,
            focus: snapshot
                .focus
                .map_or(ROOT, |i| NodeId(FIRST_CONTROL + i as u64)),
        }
    }

    struct Activation(Arc<Mutex<Described>>);

    impl ActivationHandler for Activation {
        fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
            let latest = self.0.lock().unwrap_or_else(|p| p.into_inner());
            Some(tree(&latest.0, latest.1))
        }
    }

    struct Actions {
        tx: Sender<Request>,
        wake: Box<dyn Fn() + Send>,
    }

    impl ActionHandler for Actions {
        fn do_action(&mut self, request: ActionRequest) {
            let Some(index) = request.target_node.0.checked_sub(FIRST_CONTROL) else {
                return;
            };
            let index = index as usize;
            let request = match request.action {
                Action::Click => Request::Press(index),
                Action::Focus => Request::Focus(index),
                _ => return,
            };
            if self.tx.send(request).is_ok() {
                (self.wake)();
            }
        }
    }

    struct Deactivation;

    impl DeactivationHandler for Deactivation {
        fn deactivate_accessibility(&mut self) {}
    }
}
