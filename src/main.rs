mod steam;

use std::env;
use std::fs;
use std::fs::File;
use std::path::PathBuf;
use std::process;
use std::process::{Command, Stdio};
use steam::SteamGame;

const LAUNCH_STR: &str = "launch";

type Game = SteamGame;
const DISPLAY_WIDTH: usize = 125;
const HOURS_COL_WIDTH: usize = 8;
const COL_SPACER: &str = "  ";

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

fn get_menu_selection(games: &[Game]) -> MenuChoice {
    let title_width = DISPLAY_WIDTH.saturating_sub(HOURS_COL_WIDTH + COL_SPACER.len());
    let menu_rows: Vec<(String, Game)> = games
        .iter()
        .cloned()
        .map(|game| {
            let title = truncate_title(&game.name, title_width);
            let hours = format_hours(game.playtime_minutes);
            let row = format!(
                "{:<title_width$}{COL_SPACER}{:>HOURS_COL_WIDTH$}",
                title, hours
            );
            (row, game)
        })
        .collect();
    let formatted = menu_rows
        .iter()
        .map(|(row, _)| row.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    // Prewrite rofi input, then attach it as stdin at spawn time.
    let input_path = env::temp_dir().join(format!("rofi-steam-input-{}.txt", process::id()));
    let _ = fs::write(&input_path, formatted.as_bytes());
    let stdin_file = File::open(&input_path).expect("failed to prepare rofi stdin");

    let output = Command::new("rofi")
        .arg("-monitor")
        .arg("1")
        .arg("-i")
        .arg("-dmenu")
        .arg("-sync")
        .arg("-p")
        .arg(LAUNCH_STR)
        .stdin(Stdio::from(stdin_file))
        .stdout(Stdio::piped())
        .output()
        .expect("failed to run rofi");
    let _ = fs::remove_file(&input_path);
    let selected = String::from_utf8_lossy(&output.stdout)
        .lines()
        .last()
        .unwrap_or("")
        .to_string();

    for (row, game) in menu_rows {
        if selected == row {
            return MenuChoice::Launch(game);
        }
    }

    MenuChoice::None
}

fn format_hours(playtime_minutes: u64) -> String {
    format!("{}h", playtime_minutes / 60)
}

fn truncate_title(title: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let count = title.chars().count();
    if count <= max_chars {
        return title.to_string();
    }
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }
    title.chars().take(max_chars - 3).collect::<String>() + "..."
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
    let (games_list, used_cached_data) = match read_cache() {
        Some(cached) => (cached, true),
        None => (refresh_cache_sync(), false),
    };

    match get_menu_selection(&games_list) {
        MenuChoice::Launch(game) => {
            println!("{}", game.name);
            launch_game(&game.appid);
        }
        MenuChoice::None => {}
    }

    if used_cached_data {
        let _ = refresh_cache_sync();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_hours_in_hours_unit() {
        assert_eq!(format_hours(0), "0h");
        assert_eq!(format_hours(90), "1h");
    }

    #[test]
    fn truncates_long_titles_with_ellipsis() {
        assert_eq!(truncate_title("abcdef", 6), "abcdef");
        assert_eq!(truncate_title("abcdefg", 6), "abc...");
        assert_eq!(truncate_title("abcdef", 2), "..");
    }
}
