//! Scoring/parsing behavior, exercised against the real embedded game
//! configs (`games/*.cue` → `engine::config::registry()`). Pure logic, no
//! Worker runtime.

use std::collections::BTreeMap;

use scoretracker::engine::config;
use scoretracker::engine::error::EngineError;
use scoretracker::engine::state::{self, GameData};

fn entries(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

#[test]
fn phase10_token_map_and_lowest_leads() {
    let game = config::game("phase10").expect("phase10 exists");
    let mut d = GameData::new(game);
    // Jon: 3,7 -> 5+5 = 10. Jessica: wild,12 -> 25+10 = 35.
    state::apply_round(
        &mut d,
        game,
        &entries(&[("Jon", "3, 7"), ("Jessica", "wild, 12")]),
        None,
    )
    .expect("round applies");
    assert_eq!(d.totals["Jon"], 10);
    assert_eq!(d.totals["Jessica"], 35);
    assert_eq!(d.leaders, vec!["Jon".to_owned()]); // lowest total leads
}

#[test]
fn phase10_flexible_input_and_no_cards() {
    let game = config::game("phase10").expect("phase10 exists");
    let mut d = GameData::new(game);
    state::apply_round(
        &mut d,
        game,
        &entries(&[("Jon", "wild, 5, 5"), ("Jessica", "no cards")]),
        None,
    )
    .expect("round applies");
    assert_eq!(d.totals["Jon"], 35); // 25 + 5 + 5
    assert_eq!(d.totals["Jessica"], 0); // "no cards" -> zero
}

#[test]
fn tic_deck_values_wild_and_per_tic_adjustment() {
    let game = config::game("tic").expect("tic exists");
    let mut d = GameData::new(game);
    // Jon goes out (no cards) and tics; Jessica holds king(10)+5+wild(50)=65.
    state::apply_round(
        &mut d,
        game,
        &entries(&[("Jon", ""), ("Jessica", "king, 5, wild")]),
        Some("Jon".to_owned()),
    )
    .expect("round applies");
    assert_eq!(d.totals["Jessica"], 65);
    assert_eq!(d.totals["Jon"], -5); // 0 cards + one tic (-5)
    assert_eq!(d.leaders, vec!["Jon".to_owned()]);

    // Second round: Jon tics again; Jessica holds an ace (20).
    state::apply_round(
        &mut d,
        game,
        &entries(&[("Jon", "no cards"), ("Jessica", "ace")]),
        Some("Jon".to_owned()),
    )
    .expect("round applies");
    assert_eq!(d.totals["Jon"], -10); // two tics
    assert_eq!(d.totals["Jessica"], 85);
}

#[test]
fn skipbo_tallies_wins_highest_leads() {
    let game = config::game("skipbo").expect("skipbo exists");
    let mut d = GameData::new(game);
    state::apply_match(&mut d, game, "Jon").expect("win");
    state::apply_match(&mut d, game, "Jon").expect("win");
    state::apply_match(&mut d, game, "Jessica").expect("win");
    assert_eq!(d.totals["Jon"], 2);
    assert_eq!(d.totals["Jessica"], 1);
    assert_eq!(d.leaders, vec!["Jon".to_owned()]); // most wins
}

#[test]
fn undo_removes_last_round_and_recomputes() {
    let game = config::game("phase10").expect("phase10 exists");
    let mut d = GameData::new(game);
    state::apply_round(&mut d, game, &entries(&[("Jon", "5")]), None).expect("round");
    state::apply_round(&mut d, game, &entries(&[("Jon", "5")]), None).expect("round");
    assert_eq!(d.totals["Jon"], 10);
    state::undo(&mut d, game).expect("undo");
    assert_eq!(d.totals["Jon"], 5);
    state::undo(&mut d, game).expect("undo");
    assert!(d.rounds.is_empty());
    assert!(d.leaders.is_empty());
    assert_eq!(
        state::undo(&mut d, game).unwrap_err(),
        EngineError::NothingToUndo
    );
}

#[test]
fn reset_clears_and_can_change_roster() {
    let game = config::game("phase10").expect("phase10 exists");
    let mut d = GameData::new(game);
    state::apply_round(&mut d, game, &entries(&[("Jon", "5")]), None).expect("round");
    state::reset(
        &mut d,
        game,
        Some(vec!["Ada".to_owned(), "Bo".to_owned(), "Cy".to_owned()]),
        None,
    );
    assert_eq!(d.players, vec!["Ada", "Bo", "Cy"]);
    assert!(d.rounds.is_empty());
    assert_eq!(d.totals["Ada"], 0);
    assert!(d.leaders.is_empty());
}

#[test]
fn invalid_token_errors_and_does_not_record_round() {
    let game = config::game("phase10").expect("phase10 exists");
    let mut d = GameData::new(game);
    let err = state::apply_round(&mut d, game, &entries(&[("Jon", "banana")]), None).unwrap_err();
    assert!(matches!(err, EngineError::UnknownToken { .. }));
    assert!(d.rounds.is_empty());
}

#[test]
fn award_on_a_game_without_one_is_rejected() {
    let game = config::game("phase10").expect("phase10 exists");
    let mut d = GameData::new(game);
    let err = state::apply_round(
        &mut d,
        game,
        &entries(&[("Jon", "5")]),
        Some("Jon".to_owned()),
    )
    .unwrap_err();
    assert_eq!(err, EngineError::AwardNotSupported);
}

#[test]
fn unknown_player_and_wrong_model_are_rejected() {
    let skipbo = config::game("skipbo").expect("skipbo exists");
    let mut d = GameData::new(skipbo);
    assert_eq!(
        state::apply_match(&mut d, skipbo, "Nobody").unwrap_err(),
        EngineError::UnknownPlayer {
            player: "Nobody".to_owned()
        }
    );
    // roundPoints action on a matchWins game and vice versa.
    assert_eq!(
        state::apply_round(&mut d, skipbo, &entries(&[]), None).unwrap_err(),
        EngineError::WrongModel
    );
    let phase10 = config::game("phase10").expect("phase10 exists");
    let mut d2 = GameData::new(phase10);
    assert_eq!(
        state::apply_match(&mut d2, phase10, "Jon").unwrap_err(),
        EngineError::WrongModel
    );
}

#[test]
fn tic_wild_rank_auto_scores_in_a_number_round() {
    let game = config::game("tic").expect("tic exists");
    let mut d = GameData::new(game);
    assert_eq!(d.next_wild.as_deref(), Some("3")); // round 1 wild
                                                   // Round 1: 3s are wild. Jon holds two 3s (wild, 50 each) + a 7.
    state::apply_round(
        &mut d,
        game,
        &entries(&[("Jon", "3, 3, 7"), ("Jessica", "no cards")]),
        None,
    )
    .expect("round");
    assert_eq!(d.totals["Jon"], 107); // 50 + 50 + 7
    assert_eq!(d.rounds[0].wild.as_deref(), Some("3"));
    assert_eq!(d.next_wild.as_deref(), Some("4")); // round 2 wild
}

#[test]
fn tic_wild_overrides_face_card_in_a_face_round() {
    let game = config::game("tic").expect("tic exists");
    let mut d = GameData::new(game);
    // Advance to round 9 (index 8), where Jacks are wild.
    for _ in 0..8 {
        state::apply_round(&mut d, game, &entries(&[]), None).expect("round");
    }
    assert_eq!(d.next_wild.as_deref(), Some("j"));
    // Jack -> 50 (wild), Queen/King -> 10 (face, not wild), Ace -> 20.
    state::apply_round(
        &mut d,
        game,
        &entries(&[("Jon", "jack, queen, king, ace")]),
        None,
    )
    .expect("round");
    assert_eq!(d.totals["Jon"], 90); // 50 + 10 + 10 + 20
}

#[test]
fn tic_dealer_rotates_from_chosen_first_dealer() {
    let game = config::game("tic").expect("tic exists");
    let mut d = GameData::new(game);
    assert_eq!(d.next_dealer.as_deref(), Some("Jon")); // default: players[0]
    state::reset(&mut d, game, None, Some("Jessica".to_owned()));
    assert_eq!(d.next_dealer.as_deref(), Some("Jessica")); // round 1
    state::apply_round(&mut d, game, &entries(&[]), None).expect("round");
    assert_eq!(d.rounds[0].dealer.as_deref(), Some("Jessica"));
    assert_eq!(d.next_dealer.as_deref(), Some("Jon")); // round 2 rotates
    state::apply_round(&mut d, game, &entries(&[]), None).expect("round");
    assert_eq!(d.next_dealer.as_deref(), Some("Jessica")); // round 3
}

#[test]
fn tic_caps_at_eleven_rounds() {
    let game = config::game("tic").expect("tic exists");
    let mut d = GameData::new(game);
    for _ in 0..11 {
        state::apply_round(&mut d, game, &entries(&[]), None).expect("round");
    }
    assert!(d.complete);
    assert!(d.next_wild.is_none());
    assert!(d.next_dealer.is_none());
    assert_eq!(
        state::apply_round(&mut d, game, &entries(&[]), None).unwrap_err(),
        EngineError::GameComplete
    );
    // Undo re-opens the final round.
    state::undo(&mut d, game).expect("undo");
    assert!(!d.complete);
    assert_eq!(d.next_wild.as_deref(), Some("k")); // round 11 wild
}

#[test]
fn games_without_dealer_or_wild_expose_neither() {
    for id in ["phase10", "skipbo"] {
        let game = config::game(id).expect("game exists");
        let d = GameData::new(game);
        assert!(d.next_dealer.is_none(), "{id} should have no dealer");
        assert!(d.next_wild.is_none(), "{id} should have no wild");
        assert!(!d.complete, "{id} should never be 'complete'");
    }
}
