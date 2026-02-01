use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        if let Ok(metadata) = fs::metadata(path) {
            return metadata.is_file() && (metadata.permissions().mode() & 0o111 != 0);
        }
        false
    }

    #[cfg(not(unix))]
    {
        fs::metadata(path).map(|m| m.is_file()).unwrap_or(false)
    }
}

fn find_in_path(cmd: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(cmd);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn main() {
    let mut input = String::new();

    loop {
        print!("$ ");
        io::stdout().flush().unwrap();

        input.clear();
        let bytes = io::stdin().read_line(&mut input).unwrap();
        if bytes == 0 {
            break; // EOF
        }

        let mut parts = input.split_whitespace();
        let Some(cmd) = parts.next() else {
            continue;
        };

        if cmd == "exit" {
            break;
        }

        if cmd == "echo" {
            let output = parts.collect::<Vec<_>>().join(" ");
            println!("{output}");
            continue;
        }

        if cmd == "type" {
            if let Some(query) = parts.next() {
                match query {
                    "echo" | "exit" | "type" => {
                        println!("{query} is a shell builtin");
                    }
                    _ => match find_in_path(query) {
                        Some(path) => {
                            println!("{query} is {}", path.display());
                        }
                        None => {
                            println!("{query}: not found");
                        }
                    },
                }
            }
            continue;
        }

        println!("{cmd}: command not found");
    }
}
