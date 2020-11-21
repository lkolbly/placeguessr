use crate::location::Location;
use crate::location_generator::{LocationGenerator, LocationGeneratorTrait};
use crate::DistanceKm;
use crate::PlayerId;
use serde_derive::Serialize;
use std::collections::HashMap;
//use std::time::Instant;

type Points = usize;

/// We want approximately the following place numbers
/// Maximum score: 10,000pts, <1km
/// Same city: 5,000pts, <20km
/// Same state (Texas): 2,500pts, <200km
/// Same US-sized country (US, China, Australia, Mexico, Europe): 1,000pts, <2,000km
/// Same planet: 10pts, <10,000km
fn distance_to_points(distance: DistanceKm) -> usize {
    if distance < 1.0 {
        10_000
    } else if distance > 20_000.0 {
        10
    } else {
        (10_000.0 - 3174.471323 * distance.ln().sqrt()) as usize
    }
}

#[derive(Debug, PartialEq)]
pub enum Error {
    CannotAddPlayer,
    GameOver,
}

type Result<T> = std::result::Result<T, Error>;

#[derive(Serialize, Debug)]
pub struct GuessResult {
    guess: Location,
    actual: Location,
    distance: DistanceKm,
    points_gained: Points,
    new_points: Points,
}

#[derive(Serialize, PartialEq, Debug)]
pub enum PlayerState {
    /// Player is in the game, waiting for other players to join
    Joined,

    /// Player has a street view, but has not yet guessed where they are
    Guessing,

    /// Player has guessed where they are, and is waiting for other players to guess & game to move on
    GuessResults,

    /// The game is complete, and player is viewing the final results
    FinalResults,
}

#[derive(Serialize)]
pub struct Player {
    pub name: String,
    pub points: usize,
    pub state: PlayerState,
    pub guesses: Vec<Location>,
}

#[derive(Serialize)]
pub struct Game {
    players: HashMap<PlayerId, Player>,
    locations: Vec<Location>,
    locations_remaining: usize,
    dataset: String,
}

impl Game {
    pub fn new(num_locations: usize, generator: &LocationGenerator, dataset: &str) -> Game {
        let locations: Vec<_> = (0..num_locations)
            .map(|_| generator.sample_from_dataset(dataset))
            .collect();
        // These are some test-case locations that have proven tricky
        /*let locations = vec![
            Location {
                latitude: 39.533203125,
                longitude: -104.85222625732422,
            },
            Location {
                latitude: 37.06867218017578,
                longitude: -121.54496002197266,
            },
            Location {
                latitude: 50.437025,
                longitude: 51.682431,
            },
            Location {
                latitude: 34.04820251464844,
                longitude: 8.216394424438477,
            },
            Location {
                latitude: 36.332706451416016,
                longitude: 3.49965000152879,
            },
            Location {
                latitude: 11.756479263305664,
                longitude: -2.8153998851776123,
            },
            Location {
                latitude: 24.17917251586914,
                longitude: 47.30128860473633,
            },
        ];*/
        let num_locations = locations.len();
        Game {
            players: HashMap::new(),
            //state_timeout: None,
            locations: locations,
            locations_remaining: num_locations,
            dataset: dataset.to_string(),
        }
    }

    pub fn add_player(&mut self, id: PlayerId, nickname: &str) -> Result<()> {
        let guesses: Vec<Location> = vec![];
        let player = Player {
            name: nickname.to_string(),
            points: 0,
            state: PlayerState::Joined,
            guesses,
        };
        self.players.insert(id, player);
        Ok(())
    }

    pub fn get_location(&self, idx: usize) -> Location {
        self.locations[idx].clone()
    }

    pub fn get_current_location(&self) -> Location {
        self.locations[self.locations.len() - self.locations_remaining].clone()
    }

    pub fn get_locations_remaining(&self) -> usize {
        self.locations_remaining
    }

    pub fn get_player_ids(&self) -> Vec<PlayerId> {
        self.players.iter().map(|(k, _)| *k).collect()
    }

    pub fn get_player(&self, id: PlayerId) -> Option<&Player> {
        self.players.get(&id)
    }

    pub fn is_finished(&self) -> bool {
        self.locations_remaining == 0
    }

    /// True if everyone has guessed for the current location
    pub fn everyone_guessed(&self) -> bool {
        self.players
            .iter()
            .map(|(_, player)| player.state == PlayerState::GuessResults)
            .fold(true, |acc, x| acc && x)
    }

    pub fn start(&mut self) {
        for (_, player) in self.players.iter_mut() {
            player.state = PlayerState::Guessing;
        }
    }

    /// Move everybody to the next guess
    pub fn advance_guess(&mut self) -> Result<()> {
        if self.locations_remaining == 0 {
            return Err(Error::GameOver);
        }
        self.locations_remaining -= 1;
        for (_, player) in self.players.iter_mut() {
            player.state = if self.locations_remaining == 0 {
                PlayerState::FinalResults
            } else {
                PlayerState::Guessing
            };
        }
        Ok(())
    }

    /// Record a guess for the given player
    pub fn guess(&mut self, player_id: PlayerId, guess: &Location) -> Result<GuessResult> {
        if self.locations_remaining == 0 {
            return Err(Error::GameOver);
        }
        let actual = self.get_current_location(); //&self.locations[self.locations.len() - self.locations_remaining];
        let distance = actual.distance_to(&guess);
        //game.players_guessed += 1;
        let points = distance_to_points(distance);
        let mut player = self
            .players
            .get_mut(&player_id)
            .expect("Could not locate player!");
        player.points += points;
        player.state = PlayerState::GuessResults;
        player.guesses.push(guess.clone());
        //*.player_points.get_mut(&playerid).unwrap() += points;
        //*game.player_states.get_mut(&playerid).unwrap() = PlayerState::GuessResults;
        /*game.player_guesses
        .get_mut(&playerid)
        .unwrap()
        .push(guess.clone());*/
        Ok(GuessResult {
            guess: guess.clone(),
            actual: actual.clone(),
            distance: distance,
            points_gained: points,
            new_points: player.points,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::location_generator::LocationGenerator;

    #[test]
    fn test_add_player() {
        let mut game = Game::new(5, &LocationGenerator::mock(), "world");
        assert_eq!(game.add_player(1234, "MyNickname"), Ok(()));
        assert_eq!(game.get_locations_remaining(), 5);
        assert_eq!(game.get_player_ids(), vec![1234]);
        assert_eq!(game.get_player(1234).unwrap().name, "MyNickname");
        assert_eq!(game.get_player(1234).unwrap().points, 0);
    }

    #[test]
    fn test_singleplayer_game() {
        let mut game = Game::new(2, &LocationGenerator::mock(), "world");
        game.add_player(1234, "MyNickname").unwrap();
        assert_eq!(game.get_player(1234).unwrap().state, PlayerState::Joined);

        game.start();
        assert_eq!(game.everyone_guessed(), false);
        assert_eq!(game.is_finished(), false);
        assert_eq!(game.get_player(1234).unwrap().state, PlayerState::Guessing);

        let guess_result = game.guess(
            1234,
            &LocationGenerator::mock().sample_from_dataset("world"),
        );
        assert_eq!(guess_result.unwrap().points_gained, 10_000);
        assert_eq!(game.get_player(1234).unwrap().points, 10_000);
        assert_eq!(game.everyone_guessed(), true);
        assert_eq!(
            game.get_player(1234).unwrap().state,
            PlayerState::GuessResults
        );

        game.advance_guess().unwrap();
        assert_eq!(game.everyone_guessed(), false);
        assert_eq!(game.is_finished(), false);
        assert_eq!(game.get_player(1234).unwrap().state, PlayerState::Guessing);

        let guess_result = game.guess(
            1234,
            &LocationGenerator::mock().sample_from_dataset("world"),
        );
        assert_eq!(guess_result.unwrap().points_gained, 10_000);
        assert_eq!(game.get_player(1234).unwrap().points, 20_000);
        assert_eq!(game.everyone_guessed(), true);
        assert_eq!(
            game.get_player(1234).unwrap().state,
            PlayerState::GuessResults
        );

        game.advance_guess().unwrap();
        assert_eq!(game.is_finished(), true);
        assert_eq!(
            game.get_player(1234).unwrap().state,
            PlayerState::FinalResults
        );
    }
}
