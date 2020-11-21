#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use]
extern crate rocket;

use rocket::http::Status;
use rocket::http::{Cookie, Cookies};
use rocket::request::Form;
use rocket::request::FromForm;
use rocket::request::FromRequest;
use rocket::request::Outcome;
use rocket::Request;
use rocket::State;
use rocket_contrib::templates::Template;
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

mod game_logic;
mod location;
mod location_generator;

use game_logic::*;
use location::*;
use location_generator::*;

type PlayerId = usize;
type GameId = usize;

struct GuardedGame(Arc<Mutex<Game>>);
struct GuardedGameAndPid(Arc<Mutex<Game>>, PlayerId);

struct Games {
    generator: LocationGenerator,
    games: HashMap<GameId, GuardedGame>,
    players: HashMap<PlayerId, GameId>,
    next_playerid: usize,
    next_gameid: usize,
}

type GuardedGames = Arc<Mutex<Games>>;

impl Games {
    fn new(generator: LocationGenerator) -> Games {
        Games {
            generator: generator,
            games: HashMap::new(),
            players: HashMap::new(),
            next_playerid: 0,
            next_gameid: 0,
        }
    }

    fn new_playerid(&mut self) -> usize {
        self.next_playerid += 1;
        self.next_playerid
    }

    fn add_game(&mut self, game: Game) {
        self.next_gameid += 1;
        for pid in game.get_player_ids().iter() {
            self.players.insert(*pid, self.next_gameid);
        }
        self.games
            .insert(self.next_gameid, GuardedGame(Arc::new(Mutex::new(game))));
    }

    fn get_game(&mut self, playerid: usize) -> Option<GuardedGame> {
        let gameid = self.players.get(&playerid)?;
        let game = self.games.get_mut(&gameid)?;
        Some(GuardedGame(game.0.clone()))
    }
}

impl<'a, 'r> FromRequest<'a, 'r> for GuardedGameAndPid {
    type Error = ();

    fn from_request(request: &'a Request<'r>) -> Outcome<Self, Self::Error> {
        let playerid = match request.cookies().get("playerid") {
            Some(s) => match s.value().parse::<usize>() {
                Ok(pid) => pid,
                Err(_) => {
                    return Outcome::Failure((Status::BadRequest, ()));
                }
            },
            None => {
                return Outcome::Failure((Status::BadRequest, ()));
            }
        };
        log::info!("Received authentication from playerid={}", playerid);

        let db = request.guard::<State<GuardedGames>>().unwrap();
        let mut db = db.inner().lock().unwrap();
        match db.get_game(playerid) {
            Some(game) => Outcome::Success(GuardedGameAndPid(game.0, playerid)),
            None => Outcome::Failure((Status::BadRequest, ())),
        }
    }
}

#[get("/hello/<name>/<age>")]
fn hello(name: String, age: u8) -> String {
    format!("Hello, {} year old named {}!", age, name)
}

#[get("/index")]
fn index() -> Template {
    let data: HashMap<String, String> = HashMap::new();
    Template::render("index", &data)
}

#[get("/")]
fn root() -> Template {
    let data: HashMap<String, String> = HashMap::new();
    Template::render("index", &data)
}

#[derive(FromForm)]
struct CreateGame {
    place: String,
}

#[derive(Serialize)]
struct PlayGameContext {
    api_key: String,
    location: Location,
    locations_remaining: usize,
}

#[derive(Serialize)]
struct ActualAndGuess {
    actual: Location,
    guess: Location,
}

#[derive(Serialize)]
struct GameOverContext {
    api_key: String,
    results: Vec<ActualAndGuess>,
    score: usize,
}

#[derive(Deserialize)]
struct GoogleAuthentication {
    api_key: String,
}

fn render_playgame(auth: &GoogleAuthentication, game: &mut Game, playerid: usize) -> Template {
    let player = game.get_player(playerid).unwrap();
    if player.state == PlayerState::Guessing {
        let context = PlayGameContext {
            api_key: auth.api_key.clone(),
            location: game.get_current_location(),
            locations_remaining: game.get_locations_remaining(),
        };
        Template::render("playgame", &context)
    } else {
        let mut results = vec![];
        for i in 0..player.guesses.len() {
            results.push(ActualAndGuess {
                guess: player.guesses[i].clone(),
                actual: game.get_location(i),
            });
        }
        let context = GameOverContext {
            api_key: auth.api_key.clone(),
            results: results,
            score: player.points,
        };
        Template::render("gameover", &context)
    }
}

#[post("/create-game", data = "<input>")]
fn create_game(
    db: State<GuardedGames>,
    google_auth: State<GoogleAuthentication>,
    mut cookies: Cookies,
    input: Form<CreateGame>,
) -> Template {
    let mut db = db.inner().lock().unwrap();
    let mut game = Game::new(5, &db.generator, &input.place);

    let playerid = db.new_playerid();
    game.add_player(playerid, "Player").unwrap();

    db.add_game(game);

    let game = db.get_game(playerid).unwrap();
    let mut game = game.0.lock().unwrap();

    // TODO: Only for single-player games
    game.start();

    let cookie = Cookie::build("playerid", format!("{}", playerid)).finish();
    cookies.add(cookie);

    render_playgame(&google_auth, &mut game, playerid)
}

#[get("/play-round")]
fn play_round(
    google_auth: State<GoogleAuthentication>,
    game: GuardedGameAndPid,
    cookies: Cookies,
) -> Template {
    let playerid = game.1;
    let mut game = game.0.lock().unwrap();
    render_playgame(&google_auth, &mut game, playerid)
}

#[derive(FromForm)]
struct LocationGuess {
    lat: f64,
    lon: f64,
}

#[derive(Serialize)]
struct GuessResultContext {
    api_key: String,
    result: GuessResult,
    locations_remaining: usize,
}

#[post("/guess", data = "<guess>")]
fn guess(
    google_auth: State<GoogleAuthentication>,
    game: GuardedGameAndPid,
    cookies: Cookies,
    guess: Form<LocationGuess>,
) -> Template {
    let playerid = game.1;
    let mut game = game.0.lock().unwrap();
    log::info!("Received guess attempt from playerid={}", playerid);
    let guess = Location {
        latitude: guess.lat,
        longitude: guess.lon,
    };
    let guess_result = game.guess(playerid, &guess).unwrap();
    log::info!(
        "Guess result for playerid {} was {:?}",
        playerid,
        guess_result
    );

    // Show guess results
    let data = GuessResultContext {
        api_key: google_auth.api_key.clone(),
        result: guess_result,
        locations_remaining: game.get_locations_remaining(),
    };
    Template::render("guess_result", &data)
}

#[get("/advance-guess")]
fn advance_guess(
    google_auth: State<GoogleAuthentication>,
    game: GuardedGameAndPid,
    cookies: Cookies,
) -> Template {
    let playerid = game.1;
    let mut game = game.0.lock().unwrap();

    game.advance_guess().unwrap();
    render_playgame(&google_auth, &mut game, playerid)
}

#[get("/game-poller")]
fn game_poller(db: State<GuardedGames>, cookies: Cookies) -> String {
    let playerid = cookies.get("playerid").unwrap().value();
    let playerid = playerid.parse::<usize>().unwrap();
    let mut db = db.inner().lock().unwrap();
    //serde_json::to_string(db.get_game(playerid)).unwrap()
    "Test".to_string()
}

#[get("/random/<dataset>")]
fn random(
    db: State<GuardedGames>,
    google_auth: State<GoogleAuthentication>,
    dataset: String,
) -> Template {
    let db = db.inner().lock().unwrap();
    let context = PlayGameContext {
        api_key: google_auth.api_key.clone(),
        location: db.generator.sample_from_dataset(&dataset),
        locations_remaining: 0,
    };
    Template::render("playgame", &context)
}

fn rocket(
    google_auth: GoogleAuthentication,
    root: &'static str,
    location_gen: LocationGenerator,
) -> rocket::Rocket {
    let db = Arc::new(Mutex::new(Games::new(location_gen)));
    rocket::ignite()
        .mount(
            root,
            routes![
                root,
                index,
                hello,
                create_game,
                guess,
                game_poller,
                play_round,
                advance_guess,
                random,
            ],
        )
        .attach(Template::fairing())
        .manage(db)
        .manage(google_auth)
}

fn main() {
    //env_logger::init();
    let location_gen =
        LocationGenerator::from_datafile(&[("mcdonalds", "mcdonalds.dat"), ("world", "roads.dat")]);

    let google_auth: GoogleAuthentication =
        serde_yaml::from_str(&std::fs::read_to_string("keys.yaml").unwrap()).unwrap();

    rocket(google_auth, "/placeguessr", location_gen).launch();
}

#[cfg(test)]
mod test {
    use super::rocket;
    use crate::GoogleAuthentication;
    use crate::LocationGenerator;
    use rocket::http::ContentType;
    use rocket::http::Status;
    use rocket::local::Client;

    fn mkrocket() -> rocket::Rocket {
        let mock_auth = GoogleAuthentication {
            api_key: "1234".to_string(),
        };
        rocket(mock_auth, "/", LocationGenerator::mock())
    }

    #[test]
    fn load_index() {
        let client = Client::new(mkrocket()).unwrap();
        let response = client.get("/").dispatch();
        assert_eq!(response.status(), Status::Ok);
    }

    #[test]
    fn create_singleplayer_game() {
        let client = Client::new(mkrocket()).unwrap();
        let response = client
            .post("/create-game")
            .header(ContentType::Form)
            .body("place=world")
            .dispatch();
        assert_eq!(response.status(), Status::Ok);
    }

    #[test]
    fn play_singleplayer_game() {
        let client = Client::new(mkrocket()).unwrap();
        let response = client
            .post("/create-game")
            .header(ContentType::Form)
            .body("place=world")
            .dispatch();
        assert_eq!(response.status(), Status::Ok);

        // Guess 5 times
        for _ in 0..5 {
            let response = client
                .post("/guess")
                .header(ContentType::Form)
                .body("lat=30&lon=-90")
                .dispatch();
            assert_eq!(response.status(), Status::Ok);

            let response = client.get("/advance-guess").dispatch();
            assert_eq!(response.status(), Status::Ok);
        }
    }
}
