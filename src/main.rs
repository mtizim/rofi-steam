mod steam;

use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use steam::SteamGame;

const LAUNCH_STR: &str = "launch";

type Game = SteamGame;

#[derive(Debug)]
enum MenuChoice {
    Launch(Game),
    None,
}

fn cache_path() -> PathBuf {
    let home = env::var("HOME").unwrap_or_else(|_| String::from("~"));
    PathBuf::from(home).join(".launchablegames")
}

fn read_cache() -> Option<Vec<Game>> {
    let content = fs::read_to_string(cache_path()).ok()?;
    let games: Vec<Game> = serde_json::from_str(&content).ok()?;
    if games.is_empty() { None } else { Some(games) }
}

fn write_cache(games: &[Game]) {
    if let Ok(content) = serde_json::to_string(games) {
        let _ = fs::write(cache_path(), content);
    }
}

fn refresh_cache_sync() -> Vec<Game> {
    let games = steam::installed_games().unwrap_or_default();
    write_cache(&games);
    games
}

fn refresh_cache_async() {
    thread::spawn(|| {
        let games = steam::installed_games().unwrap_or_default();
        write_cache(&games);
    });
}

fn get_menu_selection(games: &[Game]) -> MenuChoice {
    let formatted = games
        .iter()
        .map(|game| game.name.clone())
        .collect::<Vec<_>>()
        .join("\n");

    let mut rofi = Command::new("rofi")
        .arg("-monitor")
        .arg("1")
        .arg("-i")
        .arg("-dmenu")
        .arg("-p")
        .arg(LAUNCH_STR)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to run rofi");

    if let Some(stdin) = rofi.stdin.as_mut() {
        let _ = stdin.write_all(formatted.as_bytes());
    }

    let output = rofi.wait_with_output().expect("failed waiting for rofi");
    let selected = String::from_utf8_lossy(&output.stdout)
        .lines()
        .last()
        .unwrap_or("")
        .to_string();

    for game in games {
        if selected == game.name {
            return MenuChoice::Launch(game.clone());
        }
    }

    MenuChoice::None
}

fn launch_game(appid: &str) {
    let _ = Command::new("steam")
        .arg(format!("steam://rungameid/{appid}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

fn main() {
    let games_list = match read_cache() {
        Some(cached) => {
            refresh_cache_async();
            cached
        }
        None => refresh_cache_sync(),
    };

    match get_menu_selection(&games_list) {
        MenuChoice::Launch(game) => {
            println!("{}", game.name);
            launch_game(&game.appid);
        }
        MenuChoice::None => {}
    }
}
