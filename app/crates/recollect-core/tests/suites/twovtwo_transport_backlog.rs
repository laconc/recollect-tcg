//! The four-socket server lobby + client 6×6 render. The engine
//! (Engine::new_2v2) and the view/session layers (view_for_slot,
//! Session::apply_slot, four-view fanout) carry the 2v2 surface; these contracts
//! point at the socket plumbing and the renderer that sit on top of them.

#[test]
#[ignore = "transport contract: the server 2v2 lobby is live — `/matches?mode=2v2` mints four slot tokens (A1/B1/A2/B2) into a Lobby2v2, ws_handler routes a token to its slot, and the command loop drives Session::apply_slot fanning a per-slot TeamView to all four sockets (TeamWelcome/TeamApplied/TeamUpdate, recollect-protocol). 1v1 is postgres-authoritative; 2v2 runs in-memory."]
fn server_hosts_a_four_socket_2v2_lobby() {}

#[test]
#[ignore = "client-render contract (web): the wgpu shell draws the 6×6 TeamView (build_team_scene → WebRenderer::draw_team), with a 'Watch a 2v2' AI mode (LocalGame::new_2v2)."]
fn client_renders_the_six_by_six_team_view() {}
