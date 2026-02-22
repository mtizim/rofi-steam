use std::collections::HashSet;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SteamGame {
    pub name: String,
    pub appid: String,
}

pub fn installed_games() -> io::Result<Vec<SteamGame>> {
    let home =
        env::var("HOME").map_err(|_| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?;
    installed_games_from_root(Path::new(&home).join(".steam").as_path())
}

pub fn installed_games_from_root(steam_root: &Path) -> io::Result<Vec<SteamGame>> {
    let primary_steamapps = steam_root.join("steam").join("steamapps");
    let mut library_paths = parse_library_paths(&primary_steamapps.join("libraryfolders.vdf"))?;

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
                    games.push(game);
                }
            }
        }
    }

    games.sort_by(|a, b| a.appid.cmp(&b.appid));
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

    for line in content.lines() {
        let quoted = quoted_values(line);
        if quoted.len() < 2 {
            continue;
        }

        match quoted[0].as_str() {
            "name" => name = Some(quoted[1].clone()),
            "appid" => appid = Some(quoted[1].clone()),
            _ => {}
        }
    }

    Some(SteamGame {
        name: name?,
        appid: appid?,
    })
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
                },
                SteamGame {
                    name: "Team Fortress Classic".to_string(),
                    appid: "20".to_string(),
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
            }]
        );
    }
}
