//! End to end: a real server, real clients (headless), real simulation.

mod common;

use std::time::Duration;

use af_core::ext::net::*;
use af_core::prelude::*;
use af_net::auth::AuthUser;
use bevy::prelude::*;
use common::*;

/// The client's own unit and where it stands, for a move order.
fn a_unit_of(app: &mut App) -> (Entity, Vec2) {
    let unit = own_units(app).into_iter().next().expect("a unit");
    let pos = position(app, unit).unwrap();
    (unit, pos)
}

#[test]
fn casual_room_plays_the_same_match_on_every_client() {
    let server = start_server(None);
    let mut a = client();
    let mut b = client();
    act(&mut a, NetAction::SetName("Ann".into()));
    act(&mut b, NetAction::SetName("Bob".into()));
    connect(&mut a, &server.addr);
    connect(&mut b, &server.addr);
    assert!(!session(&a).ranked_enabled);

    act(&mut a, NetAction::CreateRoom("friends".into()));
    pump(&mut [&mut a], Duration::from_secs(5), "the room", |apps| session(apps[0]).lobby.is_some());
    act(&mut b, NetAction::RefreshRooms);
    pump(&mut [&mut b], Duration::from_secs(5), "the room list", |apps| !session(apps[0]).rooms.is_empty());
    let room = session(&b).rooms[0].clone();
    assert_eq!((room.name.as_str(), room.humans, room.playing), ("friends", 1, false));
    act(&mut b, NetAction::JoinRoom(room.id));
    pump(&mut [&mut a, &mut b], Duration::from_secs(5), "both in the room", |apps| apps.iter().all(|x| session(x).lobby.as_ref().is_some_and(|l| l.slots.len() == 2)));
    let lobby = session(&a).lobby.clone().unwrap();
    assert!(lobby.host_is_you && lobby.slots[0].you && lobby.slots[1].name == "Bob");
    act(&mut a, NetAction::SetFaction(Some("ironhold".into())));
    act(&mut b, NetAction::SetFaction(Some("dune".into())));
    act(&mut b, NetAction::SetTeam(1));
    act(&mut a, NetAction::SetOptions(RoomOptions { fog: false, rule: VictoryRule::LastStanding, ai_players: 0 }));
    pump(&mut [&mut a, &mut b], Duration::from_secs(5), "the settings", |apps| {
        session(apps[1]).lobby.as_ref().is_some_and(|l| !l.options.fog && l.slots[1].faction.as_deref() == Some("dune") && l.slots[0].faction.as_deref() == Some("ironhold"))
    });
    act(&mut b, NetAction::Start);
    pump(&mut [&mut a, &mut b], Duration::from_secs(5), "the host's refusal", |apps| session(apps[1]).error.is_some());
    act(&mut a, NetAction::Start);
    pump(&mut [&mut a, &mut b], Duration::from_secs(10), "both playing", |apps| apps.iter().all(|x| playing(x)));
    assert!(a.world().contains_resource::<Multiplayer>());
    let setup = a.world().resource::<MatchSetup>().clone();
    assert_eq!(setup.players.iter().map(|p| p.team).collect::<Vec<_>>(), vec![0, 1]);
    assert_eq!(a.world().resource::<LocalPlayer>().0, Some(PlayerId(0)));
    assert_eq!(b.world().resource::<LocalPlayer>().0, Some(PlayerId(1)));
    assert_eq!(b.world().resource::<MatchSetup>().seed, setup.seed);

    // Both clocks run on the server's turns.
    pump(&mut [&mut a, &mut b], Duration::from_secs(15), "one second of play", |apps| apps.iter().all(|x| tick(x) >= 30));
    // Ann orders a unit across the map; it moves on Bob's screen too.
    let (unit, start) = a_unit_of(&mut a);
    assert_eq!(position(&b, unit), Some(start), "the same entity ids everywhere");
    command(&mut a, PlayerCommand::new(PlayerId(0), vec![unit], CommandKind::Move(start + Vec2::new(25.0, 0.0))));
    pump(&mut [&mut a, &mut b], Duration::from_secs(15), "the unit to move on both", |apps| {
        apps.iter().all(|x| position(x, unit).is_some_and(|p| p.distance(start) > 3.0))
    });
    // A command Bob sends for Ann's unit is dropped by the sim (not his).
    command(&mut b, PlayerCommand::new(PlayerId(1), vec![unit], CommandKind::Stop));
    pump(&mut [&mut a, &mut b], Duration::from_secs(15), "four seconds of play", |apps| apps.iter().all(|x| tick(x) >= 120));
    let (common, differ) = traces_agree(&a, &b);
    assert!(common >= 90, "only {common} ticks in common");
    assert!(differ.is_empty(), "the clients disagree at ticks {differ:?}");
    assert!(!session(&a).lockstep.desynced && !session(&b).lockstep.desynced);
    assert!(session(&a).lockstep.stalled_secs < 1.0);

    // Ann leaves: she resigns, her realm falls, Bob wins, the server ends the match.
    a.world_mut().resource_mut::<NextState<GameState>>().set(GameState::Menu);
    pump(&mut [&mut a, &mut b], Duration::from_secs(40), "the match to end", |apps| session(apps[1]).result.is_some());
    assert!(!a.world().contains_resource::<Multiplayer>());
    let result = session(&b).result.clone().unwrap();
    assert_eq!(result.winning_team, Some(1));
    assert_eq!(result.forfeited, vec!["Ann".to_string()]);
    assert!(b.world().resource::<GameOutcome>().ended);
    // Back in the room, which is waiting again.
    pump(&mut [&mut a, &mut b], Duration::from_secs(5), "the room to rest", |apps| session(apps[1]).lobby.as_ref().is_some_and(|l| !l.playing));
    assert!(session(&a).lobby.as_ref().is_some_and(|l| !l.playing), "Ann left the match, not the room");
    assert_eq!(session(&a).result.as_ref().map(|r| r.winning_team), Some(Some(1)), "the leaver hears the result too");
}

#[test]
fn ranked_duel_forfeit_moves_the_ratings() {
    let auth = start_mock_auth();
    let server = start_server(Some(&auth.url));
    let accounts = server.accounts.clone().unwrap();
    accounts.add_token("tok-ann", AuthUser { id: "ann".into(), username: "Ann".into() });
    accounts.add_token("tok-bob", AuthUser { id: "bob".into(), username: "Bob".into() });
    let mut a = client();
    let mut b = client();
    connect(&mut a, &server.addr);
    connect(&mut b, &server.addr);
    assert!(session(&a).ranked_enabled);
    act(&mut a, NetAction::JoinRanked(RankedMode::Duel));
    pump(&mut [&mut a], Duration::from_secs(5), "a refusal before login", |apps| session(apps[0]).error.as_deref() == Some("log in first"));
    act(&mut a, NetAction::LogIn { username: "Ann".into(), password: "wrong-password".into() });
    pump(&mut [&mut a], Duration::from_secs(10), "the wrong password", |apps| session(apps[0]).error.as_deref() == Some("wrong username or password"));
    act(&mut a, NetAction::LogIn { username: "Ann".into(), password: "password123".into() });
    act(&mut b, NetAction::SignUp { username: "Bob".into(), password: "password123".into() });
    pump(&mut [&mut a, &mut b], Duration::from_secs(10), "both logged in", |apps| apps.iter().all(|x| session(x).account.is_some()));
    assert_eq!(session(&a).account.as_ref().unwrap().elo, 1600);
    assert_eq!(session(&b).account.as_ref().unwrap().username, "Bob");
    act(&mut a, NetAction::Leaderboard);
    pump(&mut [&mut a], Duration::from_secs(5), "the leaderboard", |apps| session(apps[0]).leaderboard.len() == 2);

    act(&mut a, NetAction::JoinRanked(RankedMode::Duel));
    pump(&mut [&mut a, &mut b], Duration::from_secs(5), "Ann waiting", |apps| session(apps[1]).ranked_slots.iter().any(|s| s.mode == RankedMode::Duel && s.waiting == 1));
    assert!(session(&a).lobby.as_ref().is_some_and(|l| l.kind == LobbyKind::Ranked(RankedMode::Duel) && l.slots[0].elo == Some(1600)));
    act(&mut b, NetAction::JoinRanked(RankedMode::Duel));
    pump(&mut [&mut a, &mut b], Duration::from_secs(10), "the ranked match", |apps| apps.iter().all(|x| playing(x)));
    assert!(a.world().resource::<Multiplayer>().ranked);
    assert_eq!(a.world().resource::<MatchSetup>().rule, VictoryRule::LastStanding);
    pump(&mut [&mut a, &mut b], Duration::from_secs(15), "a second of play", |apps| apps.iter().all(|x| tick(x) >= 30));
    // The referee drops a command for a unit the sender does not own.
    let (ann_unit, start) = a_unit_of(&mut a);
    command(&mut b, PlayerCommand::new(PlayerId(1), vec![ann_unit], CommandKind::Move(start + Vec2::new(30.0, 0.0))));
    pump(&mut [&mut a, &mut b], Duration::from_secs(15), "two more seconds", |apps| apps.iter().all(|x| tick(x) >= 90));
    assert_eq!(position(&a, ann_unit), Some(start), "Bob cannot move Ann's unit");

    // Ann leaves: forfeit. Bob wins once the grace period has passed.
    a.world_mut().resource_mut::<NextState<GameState>>().set(GameState::Menu);
    pump(&mut [&mut a, &mut b], Duration::from_secs(40), "the result", |apps| session(apps[1]).result.is_some() && session(apps[0]).result.is_some());
    let result = session(&b).result.clone().unwrap();
    assert_eq!(result.winning_team, Some(1));
    assert_eq!(result.forfeited, vec!["Ann".to_string()]);
    let change = |name: &str| result.changes.iter().find(|c| c.username == name).map(|c| (c.before, c.after)).unwrap();
    assert_eq!(change("Ann"), (1600, 1584));
    assert_eq!(change("Bob"), (1600, 1616));
    pump(&mut [&mut a, &mut b], Duration::from_secs(5), "the new ratings", |apps| session(apps[1]).account.as_ref().is_some_and(|acc| acc.elo == 1616));
    assert_eq!(session(&a).account.as_ref().map(|acc| (acc.elo, acc.losses)), Some((1584, 1)));
    let records = accounts.matches();
    assert_eq!(records.len(), 1);
    assert_eq!((records[0].mode.as_str(), records[0].winning_team, records[0].note.as_str()), ("duel", Some(1), ""));
    assert!(records[0].players.iter().any(|p| p.user_id == "bob" && p.won && p.elo_after == 1616));
    assert_eq!(accounts.player("ann").unwrap().elo, 1584);
    // The slot is free again.
    pump(&mut [&mut a], Duration::from_secs(5), "the slot", |apps| session(apps[0]).ranked_slots.iter().any(|s| s.mode == RankedMode::Duel && !s.playing && s.waiting == 0));
}

#[test]
fn a_dropped_connection_resumes_the_match() {
    let server = start_server(None);
    let mut a = client();
    let mut b = client();
    connect(&mut a, &server.addr);
    connect(&mut b, &server.addr);
    act(&mut a, NetAction::CreateRoom("drop".into()));
    pump(&mut [&mut a], Duration::from_secs(5), "the room", |apps| session(apps[0]).lobby.is_some());
    act(&mut b, NetAction::RefreshRooms);
    pump(&mut [&mut b], Duration::from_secs(5), "rooms", |apps| !session(apps[0]).rooms.is_empty());
    let id = session(&b).rooms[0].id;
    act(&mut b, NetAction::JoinRoom(id));
    pump(&mut [&mut a, &mut b], Duration::from_secs(5), "both in", |apps| session(apps[0]).lobby.as_ref().is_some_and(|l| l.slots.len() == 2));
    act(&mut a, NetAction::Start);
    pump(&mut [&mut a, &mut b], Duration::from_secs(10), "playing", |apps| apps.iter().all(|x| playing(x)));
    pump(&mut [&mut a, &mut b], Duration::from_secs(15), "a second", |apps| apps.iter().all(|x| tick(x) >= 30));

    af_net::client::simulate_connection_loss(b.world_mut());
    pump(&mut [&mut a, &mut b], Duration::from_secs(10), "Bob to notice", |apps| matches!(session(apps[1]).status, NetStatus::Failed(_)));
    let stalled_at = tick(&b);
    // Ann keeps playing meanwhile.
    pump(&mut [&mut a, &mut b], Duration::from_secs(15), "Ann to go on", |apps| tick(apps[0]) >= stalled_at + 60);
    assert!(tick(&b) <= stalled_at + 5, "Bob has no turns while he is off");
    // Bob gets back in and catches up.
    pump(&mut [&mut a, &mut b], Duration::from_secs(30), "Bob back online", |apps| session(apps[1]).status == NetStatus::Online);
    pump(&mut [&mut a, &mut b], Duration::from_secs(30), "Bob to catch up", |apps| tick(apps[1]) + 5 >= tick(apps[0]));
    let (common, differ) = traces_agree(&a, &b);
    assert!(common > 100, "only {common} ticks in common");
    assert!(differ.is_empty(), "the clients disagree at ticks {differ:?}");
}

#[test]
fn a_tampered_client_is_caught_as_desynced() {
    let server = start_server(None);
    let mut a = client();
    let mut b = client();
    connect(&mut a, &server.addr);
    connect(&mut b, &server.addr);
    act(&mut a, NetAction::CreateRoom("cheat".into()));
    pump(&mut [&mut a], Duration::from_secs(5), "the room", |apps| session(apps[0]).lobby.is_some());
    act(&mut b, NetAction::RefreshRooms);
    pump(&mut [&mut b], Duration::from_secs(5), "rooms", |apps| !session(apps[0]).rooms.is_empty());
    let id = session(&b).rooms[0].id;
    act(&mut b, NetAction::JoinRoom(id));
    pump(&mut [&mut a, &mut b], Duration::from_secs(5), "both in", |apps| session(apps[0]).lobby.as_ref().is_some_and(|l| l.slots.len() == 2));
    act(&mut a, NetAction::Start);
    pump(&mut [&mut a, &mut b], Duration::from_secs(10), "playing", |apps| apps.iter().all(|x| playing(x)));
    pump(&mut [&mut a, &mut b], Duration::from_secs(15), "a second", |apps| apps.iter().all(|x| tick(x) >= 30));
    // Bob's game moves one of his units on its own.
    let unit = own_units(&mut b).into_iter().next().unwrap();
    b.world_mut().get_mut::<Position>(unit).unwrap().0.x += 7.0;
    pump(&mut [&mut a, &mut b], Duration::from_secs(20), "the desync report", |apps| apps.iter().all(|x| session(x).lockstep.desynced));
    // With two players nobody can say which one is right; both hear who differs.
    for app in [&a, &b] {
        let notices = app.world().resource::<Notices>().list.iter().map(|n| n.text.clone()).collect::<Vec<_>>();
        assert!(notices.iter().any(|n| n.contains("Desync at tick") && n.contains("no longer match the other players")), "{notices:?}");
    }
}
