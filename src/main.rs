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

fn parse_line(input: &str) -> Vec<String> {
    #[derive(Copy, Clone)]
    enum State {
        Normal,
        Single,
        Double,
    }

    let mut args = Vec::new();
    let mut current = String::new();
    let mut state = State::Normal;
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        match state {
            State::Normal => match ch {
                '\'' => state = State::Single,
                '"' => state = State::Double,
                '\\' => {
                    if let Some(next) = chars.next() {
                        current.push(next);
                    } else {
                        current.push('\\');
                    }
                }
                c if c.is_whitespace() => {
                    if !current.is_empty() {
                        args.push(current.clone());
                        current.clear();
                    }
                }
                _ => current.push(ch),
            },
            State::Single => {
                if ch == '\'' {
                    state = State::Normal;
                } else {
                    current.push(ch);
                }
            }
            State::Double => match ch {
                '"' => state = State::Normal,
                '\\' => {
                    if let Some(next) = chars.peek().copied() {
                        if next == '"' || next == '\\' {
                            chars.next();
                            current.push(next);
                        } else {
                            current.push('\\');
                        }
                    } else {
                        current.push('\\');
                    }
                }
                _ => current.push(ch),
            },
        }
    }

    if !current.is_empty() {
        args.push(current);
    }

    args
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

        let args = parse_line(&input);
        let Some(cmd) = args.first() else {
            continue;
        };
        let args = args[1..].to_vec();

        if cmd == "exit" {
            break;
        }

        if cmd == "echo" {
            let output = args.join(" ");
            println!("{output}");
            continue;
        }

        if cmd == "pwd" {
            if let Ok(dir) = env::current_dir() {
                println!("{}", dir.display());
            }
            continue;
        }

        if cmd == "cd" {
            if let Some(target) = args.first() {
                let resolved = if target == "~" {
                    env::var_os("HOME").map(PathBuf::from)
                } else {
                    Some(PathBuf::from(target))
                };

                match resolved {
                    Some(path) => {
                        if env::set_current_dir(&path).is_err() {
                            println!("cd: {target}: No such file or directory");
                        }
                    }
                    None => {
                        println!("cd: {target}: No such file or directory");
                    }
                }
            }
            continue;
        }

        if cmd == "type" {
            if let Some(query) = args.first() {
                match query.as_str() {
                    "echo" | "exit" | "type" | "pwd" | "cd" => {
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

        if let Some(_path) = find_in_path(cmd) {
            let status = std::process::Command::new(cmd)
                .args(&args)
                .status();
            if status.is_err() {
                println!("{cmd}: command not found");
            }
            continue;
        }

        println!("{cmd}: command not found");
    }
}
