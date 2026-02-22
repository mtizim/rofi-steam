use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SteamGame {
    pub name: String,
    pub appid: String,
    #[serde(default)]
    pub last_played: u64,
    #[serde(default)]
    pub playtime_minutes: u64,
}

pub fn installed_games() -> io::Result<Vec<SteamGame>> {
    let home =
        env::var("HOME").map_err(|_| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?;
    installed_games_from_root(Path::new(&home).join(".steam").as_path())
}

pub fn installed_games_from_root(steam_root: &Path) -> io::Result<Vec<SteamGame>> {
    let primary_steamapps = steam_root.join("steam").join("steamapps");
    let mut library_paths = parse_library_paths(&primary_steamapps.join("libraryfolders.vdf"))?;
    let playtimes = parse_playtimes(steam_root);

    if library_paths.is_empty() {
        library_paths.push(steam_root.join("steam"));
    }

    let mut seen = HashSet::new();
    let mut games = Vec::new();

    for library in library_paths {
        let steamapps = library.join("steamapps");
        if !steamapps.is_dir() {
            continue;
        }

        let entries = match fs::read_dir(&steamapps) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let filename = match path.file_name().and_then(|n| n.to_str()) {
                Some(name) => name,
                None => continue,
            };

            if !filename.starts_with("appmanifest_") || !filename.ends_with(".acf") {
                continue;
            }

            let content = match fs::read_to_string(&path) {
                Ok(content) => content,
                Err(_) => continue,
            };

            if let Some(game) = parse_appmanifest(&content) {
                if is_game_entry(&game) && seen.insert(game.appid.clone()) {
                    games.push(SteamGame {
                        playtime_minutes: playtimes.get(&game.appid).copied().unwrap_or(0),
                        ..game
                    });
                }
            }
        }
    }

    games.sort_by(|a, b| {
        b.last_played
            .cmp(&a.last_played)
            .then_with(|| a.appid.cmp(&b.appid))
    });
    Ok(games)
}

fn parse_library_paths(libraryfolders_file: &Path) -> io::Result<Vec<PathBuf>> {
    let content = match fs::read_to_string(libraryfolders_file) {
        Ok(content) => content,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };

    let mut paths = Vec::new();
    for line in content.lines() {
        let mut quoted = quoted_values(line);
        if quoted.len() < 2 {
            continue;
        }

        let key = quoted.remove(0);
        if key == "path" {
            let path = quoted.remove(0).replace("\\\\", "\\");
            paths.push(PathBuf::from(path));
        }
    }

    Ok(paths)
}

fn parse_appmanifest(content: &str) -> Option<SteamGame> {
    let mut name = None;
    let mut appid = None;
    let mut last_played = 0_u64;

    for line in content.lines() {
        let quoted = quoted_values(line);
        if quoted.len() < 2 {
            continue;
        }

        match quoted[0].as_str() {
            "name" => name = Some(quoted[1].clone()),
            "appid" => appid = Some(quoted[1].clone()),
            "LastPlayed" => {
                if let Ok(parsed) = quoted[1].parse::<u64>() {
                    last_played = parsed;
                }
            }
            _ => {}
        }
    }

    Some(SteamGame {
        name: name?,
        appid: appid?,
        last_played,
        playtime_minutes: 0,
    })
}

fn parse_playtimes(steam_root: &Path) -> HashMap<String, u64> {
    let mut result = HashMap::new();
    let userdata = steam_root.join("steam").join("userdata");
    let entries = match fs::read_dir(userdata) {
        Ok(entries) => entries,
        Err(_) => return result,
    };

    for entry in entries.flatten() {
        let config = entry.path().join("config").join("localconfig.vdf");
        let content = match fs::read_to_string(config) {
            Ok(content) => content,
            Err(_) => continue,
        };

        for (appid, minutes) in parse_localconfig_playtimes(&content) {
            let current = result.entry(appid).or_insert(0);
            if minutes > *current {
                *current = minutes;
            }
        }
    }

    result
}

fn parse_localconfig_playtimes(content: &str) -> HashMap<String, u64> {
    let mut playtimes = HashMap::new();
    let mut stack: Vec<String> = Vec::new();
    let mut pending_key: Option<String> = None;

    for line in content.lines() {
        let quoted = quoted_values(line);

        if quoted.len() == 1 {
            pending_key = Some(quoted[0].clone());
        } else if quoted.len() >= 2 && quoted[0] == "Playtime" {
            if stack.len() >= 2 && stack[stack.len() - 2] == "apps" {
                if let Ok(minutes) = quoted[1].parse::<u64>() {
                    let appid = stack[stack.len() - 1].clone();
                    playtimes.insert(appid, minutes);
                }
            }
        }

        for ch in line.chars() {
            if ch == '{' {
                if let Some(key) = pending_key.take() {
                    stack.push(key);
                }
            } else if ch == '}' {
                let _ = stack.pop();
            }
        }
    }

    playtimes
}

fn is_game_entry(game: &SteamGame) -> bool {
    let name = game.name.to_ascii_lowercase();
    !name.contains("proton")
        && !name.contains("steam linux runtime")
        && !name.contains("steamworks common redistributables")
}

fn quoted_values(line: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut in_quotes = false;
    let mut current = String::new();

    for ch in line.chars() {
        if ch == '"' {
            if in_quotes {
                values.push(current.clone());
                current.clear();
                in_quotes = false;
            } else {
                in_quotes = true;
            }
            continue;
        }

        if in_quotes {
            current.push(ch);
        }
    }

    values
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("failed to create parent dirs");
        }
        fs::write(path, content).expect("failed to write file");
    }

    #[test]
    fn finds_games_across_all_libraries() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let steam_root = tmp.path().join(".steam");

        let primary = steam_root.join("steam");
        let extra = tmp.path().join("mnt").join("games");

        write_file(
            &primary.join("steamapps").join("libraryfolders.vdf"),
            &format!(
                concat!(
                    "\"libraryfolders\"\n",
                    "{{\n",
                    "  \"0\"\n",
                    "  {{\n",
                    "    \"path\"\t\"{}\"\n",
                    "  }}\n",
                    "  \"1\"\n",
                    "  {{\n",
                    "    \"path\"\t\"{}\"\n",
                    "  }}\n",
                    "}}\n"
                ),
                primary.display(),
                extra.display()
            ),
        );

        write_file(
            &primary.join("steamapps").join("appmanifest_10.acf"),
            concat!(
                "\"AppState\"\n",
                "{\n",
                "  \"appid\"\t\"10\"\n",
                "  \"name\"\t\"Counter-Strike\"\n",
                "}\n"
            ),
        );

        write_file(
            &extra.join("steamapps").join("appmanifest_20.acf"),
            concat!(
                "\"AppState\"\n",
                "{\n",
                "  \"appid\"\t\"20\"\n",
                "  \"name\"\t\"Team Fortress Classic\"\n",
                "}\n"
            ),
        );

        let games = installed_games_from_root(&steam_root).expect("failed to load games");

        assert_eq!(
            games,
            vec![
                SteamGame {
                    name: "Counter-Strike".to_string(),
                    appid: "10".to_string(),
                    last_played: 0,
                    playtime_minutes: 0,
                },
                SteamGame {
                    name: "Team Fortress Classic".to_string(),
                    appid: "20".to_string(),
                    last_played: 0,
                    playtime_minutes: 0,
                },
            ]
        );
    }

    #[test]
    fn falls_back_to_default_library_when_no_libraryfolders_file() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let steam_root = tmp.path().join(".steam");

        let primary_manifest = steam_root
            .join("steam")
            .join("steamapps")
            .join("appmanifest_730.acf");

        write_file(
            &primary_manifest,
            concat!(
                "\"AppState\"\n",
                "{\n",
                "  \"appid\"\t\"730\"\n",
                "  \"name\"\t\"Counter-Strike 2\"\n",
                "}\n"
            ),
        );

        let games = installed_games_from_root(&steam_root).expect("failed to load games");

        assert_eq!(
            games,
            vec![SteamGame {
                name: "Counter-Strike 2".to_string(),
                appid: "730".to_string(),
                last_played: 0,
                playtime_minutes: 0,
            }]
        );
    }

    #[test]
    #[ignore = "depends on local ~/.steam contents"]
    fn reads_installed_games_from_home_dir() {
        let games = installed_games().expect("failed to read installed games from ~/.steam");
        assert!(
            games.len() > 10,
            "expected more than 10 installed steam games, found {}",
            games.len()
        );
        for game in games {
            assert!(
                is_game_entry(&game),
                "found non-game entry in results: {} ({})",
                game.name,
                game.appid
            );
        }
    }

    #[test]
    fn filters_known_non_game_entries() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let steam_root = tmp.path().join(".steam");
        let steamapps = steam_root.join("steam").join("steamapps");

        write_file(
            &steamapps.join("appmanifest_1493710.acf"),
            concat!(
                "\"AppState\"\n",
                "{\n",
                "  \"appid\"\t\"1493710\"\n",
                "  \"name\"\t\"Proton Experimental\"\n",
                "}\n"
            ),
        );

        write_file(
            &steamapps.join("appmanifest_1628350.acf"),
            concat!(
                "\"AppState\"\n",
                "{\n",
                "  \"appid\"\t\"1628350\"\n",
                "  \"name\"\t\"Steam Linux Runtime 3.0 (sniper)\"\n",
                "}\n"
            ),
        );

        write_file(
            &steamapps.join("appmanifest_1030300.acf"),
            concat!(
                "\"AppState\"\n",
                "{\n",
                "  \"appid\"\t\"1030300\"\n",
                "  \"name\"\t\"Hollow Knight: Silksong\"\n",
                "}\n"
            ),
        );

        let games = installed_games_from_root(&steam_root).expect("failed to load games");

        assert_eq!(
            games,
            vec![SteamGame {
                name: "Hollow Knight: Silksong".to_string(),
                appid: "1030300".to_string(),
                last_played: 0,
                playtime_minutes: 0,
            }]
        );
    }

    #[test]
    fn sorts_by_last_played_descending() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let steam_root = tmp.path().join(".steam");
        let steamapps = steam_root.join("steam").join("steamapps");

        write_file(
            &steamapps.join("appmanifest_10.acf"),
            concat!(
                "\"AppState\"\n",
                "{\n",
                "  \"appid\"\t\"10\"\n",
                "  \"name\"\t\"Older Game\"\n",
                "  \"LastPlayed\"\t\"100\"\n",
                "}\n"
            ),
        );

        write_file(
            &steamapps.join("appmanifest_20.acf"),
            concat!(
                "\"AppState\"\n",
                "{\n",
                "  \"appid\"\t\"20\"\n",
                "  \"name\"\t\"Newer Game\"\n",
                "  \"LastPlayed\"\t\"200\"\n",
                "}\n"
            ),
        );

        let games = installed_games_from_root(&steam_root).expect("failed to load games");
        let names: Vec<&str> = games.iter().map(|g| g.name.as_str()).collect();

        assert_eq!(names, vec!["Newer Game", "Older Game"]);
        assert_eq!(games[0].last_played, 200);
        assert_eq!(games[1].last_played, 100);
        assert_eq!(games[0].playtime_minutes, 0);
        assert_eq!(games[1].playtime_minutes, 0);
    }

    #[test]
    fn loads_playtime_from_localconfig() {
        let tmp = tempfile::tempdir().expect("failed to create temp dir");
        let steam_root = tmp.path().join(".steam");
        let steamapps = steam_root.join("steam").join("steamapps");
        let localconfig = steam_root
            .join("steam")
            .join("userdata")
            .join("123")
            .join("config")
            .join("localconfig.vdf");

        write_file(
            &steamapps.join("appmanifest_20.acf"),
            concat!(
                "\"AppState\"\n",
                "{\n",
                "  \"appid\"\t\"20\"\n",
                "  \"name\"\t\"Newer Game\"\n",
                "  \"LastPlayed\"\t\"200\"\n",
                "}\n"
            ),
        );

        write_file(
            &localconfig,
            concat!(
                "\"UserLocalConfigStore\"\n",
                "{\n",
                "  \"Software\"\n",
                "  {\n",
                "    \"Valve\"\n",
                "    {\n",
                "      \"Steam\"\n",
                "      {\n",
                "        \"apps\"\n",
                "        {\n",
                "          \"20\"\n",
                "          {\n",
                "            \"Playtime\"\t\"321\"\n",
                "          }\n",
                "        }\n",
                "      }\n",
                "    }\n",
                "  }\n",
                "}\n"
            ),
        );

        let games = installed_games_from_root(&steam_root).expect("failed to load games");
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].appid, "20");
        assert_eq!(games[0].playtime_minutes, 321);
    }
}
