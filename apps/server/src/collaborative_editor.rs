//! Transport-neutral shared editor authority. This is an in-process core only;
//! callers still need identity authentication and project admission remotely.
use crate::editor::EditorWorld;
use anyhow::{ensure, Result};
use arkline_protocol::{
    AssetsView, EditorEventGraph, EditorGraphDebug, EditorSceneState, ExteriorDefinition,
    ExteriorDefinitionState, Request, Response, RoomLayoutDocument, RoomLayoutState, Snapshot,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);
pub const MAX_COLLABORATIVE_EDITOR_SESSIONS: usize = 32;

fn checked_next_session_id(current: u64) -> Option<u64> {
    current.checked_add(1)
}

fn next_session_id() -> Result<EditorSessionId> {
    let raw_id = NEXT_SESSION_ID
        .fetch_update(
            Ordering::Relaxed,
            Ordering::Relaxed,
            checked_next_session_id,
        )
        .map_err(|_| anyhow::anyhow!("editor session ID space exhausted"))?;
    Ok(EditorSessionId(raw_id))
}

/// Opaque handle for one peer attached to a shared editor authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EditorSessionId(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollaborationRole {
    Host,
    Editor,
    Viewer,
}

struct Session {
    server_session: Option<String>,
    role: CollaborationRole,
    supports_docked_traversal_v2: bool,
}

/// One simulation, clock, and graph shared by every joined session.
///
/// This type deliberately has no socket/listener integration. It enforces
/// operation roles, while identity authentication and project admission remain
/// responsibilities of a future transport adapter.
pub struct CollaborativeEditorHost {
    world: EditorWorld,
    sessions: BTreeMap<EditorSessionId, Session>,
}

/// A bounded polling view of the shared authoritative state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborativeEditorSnapshot {
    pub snapshot: Snapshot,
    pub assets: Option<AssetsView>,
    pub simulation_time_ms: u64,
    pub paused: bool,
    pub graph: EditorGraphDebug,
}

impl CollaborativeEditorHost {
    pub fn open(directory: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            world: EditorWorld::open(directory)?,
            sessions: BTreeMap::new(),
        })
    }

    /// The first session must be the sole host. Hosts and editors join through
    /// the game Connect contract; viewers join with a snapshot only and create
    /// no pilot or authenticated game session.
    pub fn join(
        &mut self,
        role: CollaborationRole,
        request: Request,
    ) -> Result<(EditorSessionId, Response)> {
        self.join_with_layout_capability(role, request, 0)
    }

    /// Viewers have no game Connect request, so they negotiate scene support
    /// explicitly before a shared session is admitted. The legacy API opts out.
    pub fn join_with_layout_capability(
        &mut self,
        role: CollaborationRole,
        request: Request,
        world_layout_version: u32,
    ) -> Result<(EditorSessionId, Response)> {
        self.join_with_capabilities(role, request, world_layout_version, false)
    }

    /// A viewer negotiates traversal rendering independently of game Connect.
    /// This only changes its response projection, never the shared world state.
    pub fn join_with_capabilities(
        &mut self,
        role: CollaborationRole,
        request: Request,
        world_layout_version: u32,
        supports_docked_traversal_v2: bool,
    ) -> Result<(EditorSessionId, Response)> {
        ensure!(
            self.sessions.len() < MAX_COLLABORATIVE_EDITOR_SESSIONS,
            "editor session limit reached"
        );
        ensure!(
            if self.sessions.is_empty() {
                role == CollaborationRole::Host
            } else {
                role != CollaborationRole::Host
            },
            "the first session must be the only host"
        );
        match role {
            CollaborationRole::Host | CollaborationRole::Editor => ensure!(
                matches!(&request, Request::Connect { .. }),
                "host/editor join requires Connect"
            ),
            CollaborationRole::Viewer => ensure!(
                !self.sessions.is_empty() && matches!(&request, Request::Snapshot),
                "viewer join requires an existing host and Snapshot"
            ),
        }
        let mut server_session = None;
        let mut response = self.world.handle(&mut server_session, request);
        ensure!(response.ok, "{}", response.message);
        if role == CollaborationRole::Viewer {
            let layout = response
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.world_layout.clone())
                .unwrap_or_else(arkline_protocol::legacy_world_layout);
            layout.validate().map_err(anyhow::Error::msg)?;
            ensure!(
                world_layout_version <= arkline_protocol::WORLD_LAYOUT_SCHEMA_VERSION
                    && world_layout_version >= layout.revision.required_capability_version(),
                "viewer world layout capability is incompatible; update the client before joining this realm"
            );
            // Do not retain any session created by an unexpected request. The
            // request check above only allows Snapshot, which has no session.
            server_session = None;
        }
        let id = next_session_id()?;
        self.sessions.insert(
            id,
            Session {
                server_session,
                role,
                supports_docked_traversal_v2,
            },
        );
        project_traversal_response(&mut response, role, supports_docked_traversal_v2);
        Ok((id, response))
    }

    /// Route a normal game request through the same authority used by every peer.
    pub fn request(&mut self, id: EditorSessionId, request: Request) -> Result<Response> {
        let session = self
            .sessions
            .get_mut(&id)
            .ok_or_else(|| anyhow::anyhow!("unknown editor session"))?;
        ensure!(
            session.role != CollaborationRole::Viewer || matches!(&request, Request::Snapshot),
            "viewer sessions are read-only"
        );
        ensure!(
            !matches!(
                &request,
                Request::Connect { .. } | Request::AccountConnect { .. }
            ),
            "use join to establish a session"
        );
        let mut response = self.world.handle(&mut session.server_session, request);
        project_traversal_response(
            &mut response,
            session.role,
            session.supports_docked_traversal_v2,
        );
        Ok(response)
    }

    pub fn is_paused(&self) -> bool {
        self.world.is_paused()
    }

    pub fn snapshot(&mut self, id: EditorSessionId) -> Result<CollaborativeEditorSnapshot> {
        let response = self.request(id, Request::Snapshot)?;
        ensure!(response.ok, "{}", response.message);
        Ok(CollaborativeEditorSnapshot {
            snapshot: response
                .snapshot
                .ok_or_else(|| anyhow::anyhow!("snapshot response omitted state"))?,
            assets: response.assets,
            simulation_time_ms: self.world.time(),
            paused: self.world.is_paused(),
            graph: self.world.graph_debug(),
        })
    }

    /// Place only the authenticated actor's avatar in this development world.
    pub fn set_scene_state(&mut self, id: EditorSessionId, state: EditorSceneState) -> Result<()> {
        self.require_editor(id)?;
        let session = &self.sessions.get(&id).expect("role checked").server_session;
        self.world.set_scene_state(session, state)
    }

    pub fn room_layout_state(&self, id: EditorSessionId) -> Result<RoomLayoutState> {
        ensure!(self.sessions.contains_key(&id), "unknown editor session");
        Ok(self.world.room_layout_state())
    }

    pub fn room_layout_replace(
        &mut self,
        id: EditorSessionId,
        expected_revision: u64,
        document: RoomLayoutDocument,
    ) -> Result<RoomLayoutState> {
        self.require_editor(id)?;
        self.world.room_layout_replace(expected_revision, document)
    }
    pub fn exterior_replace(
        &mut self,
        id: EditorSessionId,
        expected_revision: u64,
        document: ExteriorDefinition,
    ) -> Result<ExteriorDefinitionState> {
        self.require_editor(id)?;
        self.world.exterior_replace(expected_revision, document)
    }
    pub fn exterior_set_enabled(
        &mut self,
        id: EditorSessionId,
        enabled: bool,
        expected_revision: u64,
    ) -> Result<()> {
        self.require_editor(id)?;
        let session = &mut self
            .sessions
            .get_mut(&id)
            .expect("role checked")
            .server_session;
        self.world
            .exterior_set_enabled(session, enabled, expected_revision)
    }

    pub fn play(&mut self, id: EditorSessionId) -> Result<()> {
        self.require_role(id, CollaborationRole::Host)?;
        self.world.play();
        Ok(())
    }
    pub fn pause(&mut self, id: EditorSessionId) -> Result<()> {
        self.require_role(id, CollaborationRole::Host)?;
        self.world.pause();
        Ok(())
    }
    pub fn step(&mut self, id: EditorSessionId, delta_ms: u64) -> Result<()> {
        self.require_role(id, CollaborationRole::Host)?;
        self.world.step(delta_ms)
    }
    pub fn advance(&mut self, id: EditorSessionId, delta_ms: u64) -> Result<()> {
        self.require_role(id, CollaborationRole::Host)?;
        self.world.advance(delta_ms)
    }
    pub fn replace_graph(
        &mut self,
        id: EditorSessionId,
        expected_revision: u64,
        graph: EditorEventGraph,
        clear_pending: bool,
    ) -> Result<()> {
        self.require_editor(id)?;
        self.world
            .graph_replace(expected_revision, graph, clear_pending)
    }

    pub fn trigger_graph(&mut self, id: EditorSessionId, node_id: u64) -> Result<()> {
        self.require_editor(id)?;
        self.world.graph_trigger(node_id)
    }
    pub fn step_graph(&mut self, id: EditorSessionId) -> Result<()> {
        self.require_editor(id)?;
        self.world.graph_step()
    }
    pub fn clear_graph(&mut self, id: EditorSessionId) -> Result<()> {
        self.require_editor(id)?;
        self.world.graph_clear()
    }

    pub fn leave(&mut self, id: EditorSessionId) -> Result<()> {
        let mut leaving = self
            .sessions
            .remove(&id)
            .ok_or_else(|| anyhow::anyhow!("unknown editor session"))?;
        if leaving.role == CollaborationRole::Host {
            // No host clock authority remains after this session leaves.
            self.world.pause();
        }
        // EditorWorld clears work bound to this actor and records the clear.
        // Never rebind queued actions to another participant implicitly.
        self.world.disconnect(&mut leaving.server_session);
        Ok(())
    }

    fn require_role(&self, id: EditorSessionId, required: CollaborationRole) -> Result<()> {
        let session = self
            .sessions
            .get(&id)
            .ok_or_else(|| anyhow::anyhow!("unknown editor session"))?;
        ensure!(
            session.role == required,
            "operation requires {required:?} role"
        );
        Ok(())
    }

    fn require_editor(&self, id: EditorSessionId) -> Result<()> {
        let session = self
            .sessions
            .get(&id)
            .ok_or_else(|| anyhow::anyhow!("unknown editor session"))?;
        ensure!(
            session.role != CollaborationRole::Viewer,
            "operation requires editor role"
        );
        Ok(())
    }
}

/// Responses own cloned public state. Legacy unauthenticated viewers cannot
/// render canonical traversal coordinates, so omit only their additive metadata.
fn project_traversal_response(
    response: &mut Response,
    role: CollaborationRole,
    supports_docked_traversal_v2: bool,
) {
    if role != CollaborationRole::Viewer || supports_docked_traversal_v2 {
        return;
    }
    fn legacy_snapshot(snapshot: &mut Snapshot) {
        snapshot.docked_traversal_capability = None;
        for pilot in &mut snapshot.pilots {
            if let Some(avatar) = &mut pilot.station_avatar {
                avatar.docked_traversal = None;
            }
        }
    }
    if let Some(snapshot) = &mut response.snapshot {
        legacy_snapshot(snapshot);
    }
    if let Some(ledger) = &mut response.ledger {
        legacy_snapshot(&mut ledger.snapshot);
    }
}

#[cfg(test)]
#[path = "collaborative_traversal_compatibility_tests.rs"]
mod traversal_compatibility_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use arkline_protocol::PROTOCOL_VERSION;

    fn path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "arkline-collaborative-editor-{}",
            rand::random::<u64>()
        ))
    }
    fn connect(pilot: &str) -> Request {
        Request::Connect {
            protocol_version: PROTOCOL_VERSION,
            pilot: pilot.to_owned(),
            resume_token: None,
            supports_room_controls: true,
            world_layout_version: 1,
            supports_fleet_hangar: true,
            supports_pioneer_interior: true,
            supports_docked_traversal_v2: false,
        }
    }

    #[test]
    fn viewers_negotiate_layout_without_creating_a_pilot_or_session_on_rejection() {
        for revision in [
            arkline_protocol::SceneLayoutRevision::LegacyV1,
            arkline_protocol::SceneLayoutRevision::OrbitalV1,
        ] {
            let directory = path();
            drop(EditorWorld::open_with_creation_scene(&directory, revision).unwrap());
            let mut authority = CollaborativeEditorHost::open(&directory).unwrap();
            let (host, _) = authority
                .join(CollaborationRole::Host, connect("LayoutHost"))
                .unwrap();
            let before = serde_json::to_value(authority.snapshot(host).unwrap().snapshot).unwrap();
            let legacy = authority.join(CollaborationRole::Viewer, Request::Snapshot);
            assert_eq!(legacy.is_ok(), revision.is_legacy());
            if let Ok((id, _)) = legacy {
                authority.leave(id).unwrap();
            }
            assert!(authority
                .join_with_layout_capability(
                    CollaborationRole::Viewer,
                    Request::Snapshot,
                    arkline_protocol::WORLD_LAYOUT_SCHEMA_VERSION + 1
                )
                .is_err());
            assert_eq!(authority.sessions.len(), 1);
            assert_eq!(
                serde_json::to_value(authority.snapshot(host).unwrap().snapshot).unwrap(),
                before
            );
            let (viewer, response) = authority
                .join_with_layout_capability(
                    CollaborationRole::Viewer,
                    Request::Snapshot,
                    arkline_protocol::WORLD_LAYOUT_SCHEMA_VERSION,
                )
                .unwrap();
            assert!(response.resume_token.is_none());
            assert_eq!(response.snapshot.unwrap().pilots.len(), 1);
            authority.leave(viewer).unwrap();
            authority.leave(host).unwrap();
            drop(authority);
            std::fs::remove_dir_all(directory).unwrap();
        }
    }

    #[test]
    fn scene_state_changes_only_the_actor_and_viewers_observe_without_mutation_rights() {
        let path = path();
        let mut authority = CollaborativeEditorHost::open(&path).unwrap();
        let (host, _) = authority
            .join(CollaborationRole::Host, connect("SceneHost"))
            .unwrap();
        let (editor, _) = authority
            .join(CollaborationRole::Editor, connect("SceneEditor"))
            .unwrap();
        let (viewer, _) = authority
            .join_with_layout_capability(
                CollaborationRole::Viewer,
                Request::Snapshot,
                arkline_protocol::WORLD_LAYOUT_SCHEMA_VERSION,
            )
            .unwrap();
        let before = authority.snapshot(viewer).unwrap();
        authority
            .set_scene_state(editor, EditorSceneState::InteriorDeck2)
            .unwrap();
        let after = authority.snapshot(viewer).unwrap();
        assert_eq!(after.simulation_time_ms, before.simulation_time_ms);
        assert!(after.paused);
        assert_eq!(
            serde_json::to_value(
                after
                    .snapshot
                    .pilots
                    .iter()
                    .find(|p| p.name == "SceneHost")
                    .unwrap()
            )
            .unwrap(),
            serde_json::to_value(
                before
                    .snapshot
                    .pilots
                    .iter()
                    .find(|p| p.name == "SceneHost")
                    .unwrap()
            )
            .unwrap(),
        );
        assert_eq!(
            after
                .snapshot
                .pilots
                .iter()
                .find(|p| p.name == "SceneEditor")
                .unwrap()
                .station_avatar
                .as_ref()
                .unwrap()
                .interior_deck,
            2
        );
        assert!(authority
            .set_scene_state(viewer, EditorSceneState::Cockpit)
            .is_err());
        assert_eq!(
            serde_json::to_value(authority.snapshot(viewer).unwrap().snapshot).unwrap(),
            serde_json::to_value(after.snapshot).unwrap(),
        );
        authority.play(host).unwrap();
        authority
            .set_scene_state(host, EditorSceneState::StationHangar)
            .unwrap();
        assert!(!authority.is_paused());
        assert!(authority
            .snapshot(editor)
            .unwrap()
            .snapshot
            .pilots
            .iter()
            .find(|p| p.name == "SceneHost")
            .unwrap()
            .station_avatar
            .is_some());
        authority.leave(editor).unwrap();
        assert!(authority
            .set_scene_state(editor, EditorSceneState::Cockpit)
            .is_err());
        authority.leave(viewer).unwrap();
        authority.leave(host).unwrap();
        drop(authority);
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn sessions_observe_one_world_clock_and_revision_conflicts_are_rejected() {
        let path = path();
        let mut host = CollaborativeEditorHost::open(&path).unwrap();
        let (a, _) = host
            .join(CollaborationRole::Host, connect("SharedPilotA"))
            .unwrap();
        let (b, _) = host
            .join(CollaborationRole::Editor, connect("SharedPilotB"))
            .unwrap();

        let a_view = host.snapshot(a).unwrap();
        let b_view = host.snapshot(b).unwrap();
        assert_eq!(a_view.simulation_time_ms, b_view.simulation_time_ms);
        assert_eq!(a_view.graph.revision, b_view.graph.revision);
        assert_eq!(a_view.snapshot.pilots.len(), 2);
        assert_eq!(b_view.snapshot.pilots.len(), 2);

        host.step(a, 50).unwrap();
        let advanced = host.snapshot(b).unwrap();
        assert_eq!(advanced.simulation_time_ms, b_view.simulation_time_ms + 50);

        host.replace_graph(a, 0, EditorEventGraph::default(), false)
            .unwrap();
        assert_eq!(host.snapshot(b).unwrap().graph.revision, 1);
        assert!(host
            .replace_graph(b, 0, EditorEventGraph::default(), true)
            .is_err());
        assert_eq!(host.snapshot(a).unwrap().graph.revision, 1);

        host.leave(a).unwrap();
        assert_eq!(host.snapshot(b).unwrap().graph.revision, 1);
        drop(host);
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn roles_are_enforced_and_leaving_executor_clears_bound_work() {
        let path = path();
        let mut host = CollaborativeEditorHost::open(&path).unwrap();
        let (owner, _) = host
            .join(CollaborationRole::Host, connect("OwnerPilot"))
            .unwrap();
        let (editor, _) = host
            .join(CollaborationRole::Editor, connect("EditorPilot"))
            .unwrap();
        let (viewer, _) = host
            .join_with_layout_capability(
                CollaborationRole::Viewer,
                Request::Snapshot,
                arkline_protocol::WORLD_LAYOUT_SCHEMA_VERSION,
            )
            .unwrap();
        assert_eq!(host.snapshot(viewer).unwrap().snapshot.pilots.len(), 2);
        assert!(host
            .join(CollaborationRole::Viewer, connect("RejectedViewerPilot"))
            .is_err());

        assert!(host.play(editor).is_err());
        assert!(host
            .replace_graph(viewer, 0, EditorEventGraph::default(), false)
            .is_err());
        assert!(host.request(viewer, Request::Undock).is_err());
        assert!(host.snapshot(viewer).is_ok());

        let graph = EditorEventGraph {
            nodes: vec![
                arkline_protocol::EditorGraphNode {
                    id: 1,
                    label: "Manual".into(),
                    position: [0.0; 2],
                    operation: arkline_protocol::EditorNodeOp::Manual,
                    breakpoint: false,
                },
                arkline_protocol::EditorGraphNode {
                    id: 2,
                    label: "Bound action".into(),
                    position: [0.0; 2],
                    operation: arkline_protocol::EditorNodeOp::StopShip,
                    breakpoint: false,
                },
            ],
            edges: vec![arkline_protocol::EditorGraphEdge {
                from: 1,
                to: 2,
                port: arkline_protocol::EditorGraphPort::Next,
            }],
            ..Default::default()
        };
        host.replace_graph(editor, 0, graph, false).unwrap();
        host.world.graph_trigger(1).unwrap();
        assert_eq!(host.snapshot(viewer).unwrap().graph.pending, 1);

        host.play(owner).unwrap();
        assert!(!host.snapshot(viewer).unwrap().paused);
        host.leave(owner).unwrap();
        let after_leave = host.snapshot(editor).unwrap();
        assert!(after_leave.paused, "host departure pauses shared time");
        assert_eq!(after_leave.graph.pending, 0);
        assert!(after_leave
            .graph
            .trace
            .iter()
            .any(|trace| trace.kind == "clear"));
        assert!(
            host.play(editor).is_err(),
            "host role does not transfer on leave"
        );
        assert!(host.advance(editor, 50).is_err());

        drop(host);
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn session_limit_and_id_exhaustion_fail_closed() {
        assert!(checked_next_session_id(u64::MAX).is_none());
        let path = path();
        let mut host = CollaborativeEditorHost::open(&path).unwrap();
        host.join(CollaborationRole::Host, connect("CapacityHost"))
            .unwrap();
        for index in 1..MAX_COLLABORATIVE_EDITOR_SESSIONS {
            host.join(
                CollaborationRole::Editor,
                connect(&format!("CapacityPilot{index}")),
            )
            .unwrap();
        }
        assert!(host
            .join_with_layout_capability(
                CollaborationRole::Viewer,
                Request::Snapshot,
                arkline_protocol::WORLD_LAYOUT_SCHEMA_VERSION
            )
            .is_err());
        drop(host);
        std::fs::remove_dir_all(path).unwrap();
    }
}
