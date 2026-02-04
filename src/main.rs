use std::env;
use std::fs;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

#[derive(Clone)]
struct ParsedToken {
    text: String,
    quoted: bool,
}

fn parse_line(input: &str) -> Vec<ParsedToken> {
    #[derive(Copy, Clone)]
    enum State {
        Normal,
        Single,
        Double,
    }

    let mut args = Vec::new();
    let mut current = String::new();
    let mut current_quoted = false;
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
                        current_quoted = true;
                    } else {
                        current.push('\\');
                        current_quoted = true;
                    }
                }
                c if c.is_whitespace() => {
                    if !current.is_empty() {
                        args.push(ParsedToken {
                            text: current.clone(),
                            quoted: current_quoted,
                        });
                        current.clear();
                        current_quoted = false;
                    }
                }
                _ => current.push(ch),
            },
            State::Single => {
                if ch == '\'' {
                    state = State::Normal;
                } else {
                    current.push(ch);
                    current_quoted = true;
                }
            }
            State::Double => match ch {
                '"' => state = State::Normal,
                '\\' => {
                    if let Some(next) = chars.peek().copied() {
                        if next == '"' || next == '\\' {
                            chars.next();
                            current.push(next);
                            current_quoted = true;
                        } else {
                            current.push('\\');
                            current_quoted = true;
                        }
                    } else {
                        current.push('\\');
                        current_quoted = true;
                    }
                }
                _ => {
                    current.push(ch);
                    current_quoted = true;
                }
            },
        }
    }

    if !current.is_empty() {
        args.push(ParsedToken {
            text: current,
            quoted: current_quoted,
        });
    }

    args
}

#[derive(Copy, Clone)]
enum RedirectMode {
    Truncate,
    Append,
}

#[derive(Default)]
struct RedirectSpec {
    stdout: Option<(PathBuf, RedirectMode)>,
    stderr: Option<(PathBuf, RedirectMode)>,
}

fn parse_redirections(tokens: Vec<ParsedToken>) -> (Vec<String>, RedirectSpec) {
    let mut args = Vec::new();
    let mut redirects = RedirectSpec::default();
    let mut i = 0;

    while i < tokens.len() {
        let token = &tokens[i];

        let parse_op = |s: &str| -> Option<(bool, RedirectMode, String)> {
            let ops = [
                ("1>>", true, RedirectMode::Append),
                ("2>>", false, RedirectMode::Append),
                (">>", true, RedirectMode::Append),
                ("1>", true, RedirectMode::Truncate),
                ("2>", false, RedirectMode::Truncate),
                (">", true, RedirectMode::Truncate),
            ];

            for (op, is_stdout, mode) in ops {
                if s == op {
                    return Some((is_stdout, mode, String::new()));
                }
                if s.starts_with(op) && s.len() > op.len() {
                    return Some((is_stdout, mode, s[op.len()..].to_string()));
                }
            }
            None
        };

        if !token.quoted {
            if let Some((is_stdout, mode, tail)) = parse_op(&token.text) {
                let target = if tail.is_empty() {
                    if i + 1 >= tokens.len() {
                        args.push(token.text.clone());
                        i += 1;
                        continue;
                    }
                    i += 2;
                    tokens[i - 1].text.clone()
                } else {
                    i += 1;
                    tail
                };

                if is_stdout {
                    redirects.stdout = Some((PathBuf::from(target), mode));
                } else {
                    redirects.stderr = Some((PathBuf::from(target), mode));
                }
                continue;
            }
        }

        args.push(token.text.clone());
        i += 1;
    }

    (args, redirects)
}

fn open_redirect_file(path: &Path, mode: RedirectMode) -> io::Result<fs::File> {
    let mut options = OpenOptions::new();
    options.write(true).create(true);
    match mode {
        RedirectMode::Truncate => {
            options.truncate(true);
        }
        RedirectMode::Append => {
            options.append(true);
        }
    }
    options.open(path)
}

fn ensure_redirect_files(redirects: &RedirectSpec) {
    if let Some((path, mode)) = &redirects.stdout {
        let _ = open_redirect_file(path, *mode);
    }
    if let Some((path, mode)) = &redirects.stderr {
        let _ = open_redirect_file(path, *mode);
    }
}

enum OutputStream {
    Stdout,
    Stderr,
}

fn write_output(text: &str, stream: OutputStream, redirects: &RedirectSpec) {
    let redirection = match stream {
        OutputStream::Stdout => &redirects.stdout,
        OutputStream::Stderr => &redirects.stderr,
    };

    if let Some((path, mode)) = redirection {
        if let Ok(mut file) = open_redirect_file(path, *mode) {
            let _ = file.write_all(text.as_bytes());
        }
        return;
    }

    match stream {
        OutputStream::Stdout => {
            print!("{text}");
            let _ = io::stdout().flush();
        }
        OutputStream::Stderr => {
            eprint!("{text}");
            let _ = io::stderr().flush();
        }
    }
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

        let tokens = parse_line(&input);
        let (tokens, redirects) = parse_redirections(tokens);
        let Some(cmd) = tokens.first().cloned() else {
            continue;
        };
        let args = tokens[1..].to_vec();
        ensure_redirect_files(&redirects);

        if cmd == "exit" {
            break;
        }

        if cmd == "echo" {
            let output = args.join(" ");
            write_output(&format!("{output}\n"), OutputStream::Stdout, &redirects);
            continue;
        }

        if cmd == "pwd" {
            if let Ok(dir) = env::current_dir() {
                write_output(
                    &format!("{}\n", dir.display()),
                    OutputStream::Stdout,
                    &redirects,
                );
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
                            write_output(
                                &format!("cd: {target}: No such file or directory\n"),
                                OutputStream::Stderr,
                                &redirects,
                            );
                        }
                    }
                    None => {
                        write_output(
                            &format!("cd: {target}: No such file or directory\n"),
                            OutputStream::Stderr,
                            &redirects,
                        );
                    }
                }
            }
            continue;
        }

        if cmd == "type" {
            if let Some(query) = args.first() {
                match query.as_str() {
                    "echo" | "exit" | "type" | "pwd" | "cd" => {
                        write_output(
                            &format!("{query} is a shell builtin\n"),
                            OutputStream::Stdout,
                            &redirects,
                        );
                    }
                    _ => match find_in_path(query) {
                        Some(path) => {
                            write_output(
                                &format!("{query} is {}\n", path.display()),
                                OutputStream::Stdout,
                                &redirects,
                            );
                        }
                        None => {
                            write_output(
                                &format!("{query}: not found\n"),
                                OutputStream::Stdout,
                                &redirects,
                            );
                        }
                    },
                }
            }
            continue;
        }

        if let Some(_path) = find_in_path(&cmd) {
            let mut command = Command::new(&cmd);
            command.args(&args);

            if let Some((path, mode)) = &redirects.stdout {
                if let Ok(file) = open_redirect_file(path, *mode) {
                    command.stdout(Stdio::from(file));
                }
            }

            if let Some((path, mode)) = &redirects.stderr {
                if let Ok(file) = open_redirect_file(path, *mode) {
                    command.stderr(Stdio::from(file));
                }
            }

            let status = command.status();
            if status.is_err() {
                write_output(
                    &format!("{cmd}: command not found\n"),
                    OutputStream::Stdout,
                    &redirects,
                );
            }
            continue;
        }

        write_output(
            &format!("{cmd}: command not found\n"),
            OutputStream::Stdout,
            &redirects,
        );
    }
}
