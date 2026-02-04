use std::env;
#[cfg(unix)]
use std::collections::BTreeSet;
use std::fs;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[cfg(unix)]
use libc::{self, STDIN_FILENO};
#[cfg(unix)]
use std::io::Read;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

const PROMPT: &str = "$ ";
#[cfg(unix)]
const COMPLETION_BUILTINS: [&str; 2] = ["echo", "exit"];

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

#[cfg(unix)]
fn completion_matches(prefix: &str) -> Vec<String> {
    let mut matches = BTreeSet::new();

    for builtin in COMPLETION_BUILTINS {
        if builtin.starts_with(prefix) {
            matches.insert(builtin.to_string());
        }
    }

    if let Some(path_var) = env::var_os("PATH") {
        for dir in env::split_paths(&path_var) {
            let Ok(entries) = fs::read_dir(dir) else {
                continue;
            };

            for entry in entries.flatten() {
                let path = entry.path();
                if !is_executable(&path) {
                    continue;
                }

                let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };

                if name.starts_with(prefix) {
                    matches.insert(name.to_string());
                }
            }
        }
    }

    matches.into_iter().collect()
}

#[cfg(unix)]
fn longest_common_prefix(words: &[String]) -> String {
    if words.is_empty() {
        return String::new();
    }

    let mut prefix = words[0].clone();
    for word in &words[1..] {
        let mut next = String::new();
        for (a, b) in prefix.chars().zip(word.chars()) {
            if a == b {
                next.push(a);
            } else {
                break;
            }
        }
        prefix = next;
        if prefix.is_empty() {
            break;
        }
    }

    prefix
}

#[cfg(unix)]
fn ring_bell() {
    print!("\x07");
    let _ = io::stdout().flush();
}

#[cfg(unix)]
struct RawModeGuard {
    fd: i32,
    original: libc::termios,
}

#[cfg(unix)]
impl RawModeGuard {
    fn new(fd: i32) -> io::Result<Self> {
        // SAFETY: tcgetattr/tcsetattr are called with a valid tty fd (stdin in tests).
        unsafe {
            let mut original = std::mem::zeroed::<libc::termios>();
            if libc::tcgetattr(fd, &mut original) != 0 {
                return Err(io::Error::last_os_error());
            }

            let mut raw = original;
            raw.c_lflag &= !(libc::ICANON | libc::ECHO);
            raw.c_iflag &= !(libc::ICRNL | libc::IXON);
            raw.c_cc[libc::VMIN] = 1;
            raw.c_cc[libc::VTIME] = 0;

            if libc::tcsetattr(fd, libc::TCSANOW, &raw) != 0 {
                return Err(io::Error::last_os_error());
            }

            Ok(Self { fd, original })
        }
    }
}

#[cfg(unix)]
impl Drop for RawModeGuard {
    fn drop(&mut self) {
        // SAFETY: restore captured terminal attributes on the same fd.
        unsafe {
            let _ = libc::tcsetattr(self.fd, libc::TCSANOW, &self.original);
        }
    }
}

#[cfg(unix)]
fn complete_buffer(buffer: &mut String, pending_multi: &mut Option<String>) {
    if buffer.chars().any(char::is_whitespace) {
        ring_bell();
        *pending_multi = None;
        return;
    }

    let prefix = buffer.clone();
    let matches = completion_matches(&prefix);
    if matches.is_empty() {
        ring_bell();
        *pending_multi = None;
        return;
    }

    if matches.len() == 1 {
        let word = &matches[0];
        if word.len() >= prefix.len() {
            print!("{} ", &word[prefix.len()..]);
            let _ = io::stdout().flush();
            *buffer = format!("{word} ");
        }
        *pending_multi = None;
        return;
    }

    let lcp = longest_common_prefix(&matches);
    if lcp.len() > prefix.len() {
        print!("{}", &lcp[prefix.len()..]);
        let _ = io::stdout().flush();
        *buffer = lcp;
        *pending_multi = None;
        return;
    }

    if pending_multi.as_deref() == Some(prefix.as_str()) {
        print!("\r\n{}\r\n{}{}", matches.join("  "), PROMPT, buffer);
        let _ = io::stdout().flush();
        *pending_multi = None;
    } else {
        ring_bell();
        *pending_multi = Some(prefix);
    }
}

#[cfg(unix)]
fn read_user_input() -> io::Result<Option<String>> {
    let mut input = String::new();
    let mut pending_multi = None;
    let mut stdin = io::stdin();

    loop {
        let mut byte = [0_u8; 1];
        match stdin.read_exact(&mut byte) {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e),
        }

        match byte[0] {
            b'\n' | b'\r' => {
                print!("\r\n");
                let _ = io::stdout().flush();
                return Ok(Some(input));
            }
            b'\t' => {
                complete_buffer(&mut input, &mut pending_multi);
            }
            127 | 8 => {
                if !input.is_empty() {
                    input.pop();
                    print!("\x08 \x08");
                    let _ = io::stdout().flush();
                }
                pending_multi = None;
            }
            4 => {
                if input.is_empty() {
                    print!("\r\n");
                    let _ = io::stdout().flush();
                    return Ok(None);
                }
            }
            ch if ch.is_ascii_graphic() || ch == b' ' => {
                let c = ch as char;
                input.push(c);
                print!("{c}");
                let _ = io::stdout().flush();
                pending_multi = None;
            }
            _ => {}
        }
    }
}

#[cfg(not(unix))]
fn read_user_input() -> io::Result<Option<String>> {
    let mut input = String::new();
    let bytes = io::stdin().read_line(&mut input)?;
    if bytes == 0 {
        return Ok(None);
    }
    Ok(Some(input.trim_end_matches(['\r', '\n']).to_string()))
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

#[derive(Default, Clone)]
struct RedirectSpec {
    stdout: Option<(PathBuf, RedirectMode)>,
    stderr: Option<(PathBuf, RedirectMode)>,
}

#[derive(Clone)]
struct PipelineStage {
    cmd: String,
    args: Vec<String>,
    redirects: RedirectSpec,
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

fn split_pipeline(tokens: Vec<ParsedToken>) -> Vec<Vec<ParsedToken>> {
    let mut stages = Vec::new();
    let mut current = Vec::new();

    for token in tokens {
        if !token.quoted && token.text == "|" {
            stages.push(current);
            current = Vec::new();
        } else {
            current.push(token);
        }
    }

    stages.push(current);
    stages
}

fn is_builtin_command(cmd: &str) -> bool {
    matches!(cmd, "echo" | "exit" | "type" | "pwd" | "cd")
}

#[derive(Default)]
struct CommandResult {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    should_exit: bool,
}

fn run_builtin(cmd: &str, args: &[String], allow_exit: bool, apply_cd: bool) -> Option<CommandResult> {
    let mut result = CommandResult::default();

    match cmd {
        "exit" => {
            if allow_exit {
                result.should_exit = true;
            }
        }
        "echo" => {
            result.stdout = format!("{}\n", args.join(" ")).into_bytes();
        }
        "pwd" => {
            if let Ok(dir) = env::current_dir() {
                result.stdout = format!("{}\n", dir.display()).into_bytes();
            }
        }
        "cd" => {
            if apply_cd {
                if let Some(target) = args.first() {
                    let resolved = if target == "~" {
                        env::var_os("HOME").map(PathBuf::from)
                    } else {
                        Some(PathBuf::from(target))
                    };

                    match resolved {
                        Some(path) => {
                            if env::set_current_dir(&path).is_err() {
                                result.stderr =
                                    format!("cd: {target}: No such file or directory\n").into_bytes();
                            }
                        }
                        None => {
                            result.stderr =
                                format!("cd: {target}: No such file or directory\n").into_bytes();
                        }
                    }
                }
            }
        }
        "type" => {
            if let Some(query) = args.first() {
                if is_builtin_command(query) {
                    result.stdout = format!("{query} is a shell builtin\n").into_bytes();
                } else if let Some(path) = find_in_path(query) {
                    result.stdout = format!("{query} is {}\n", path.display()).into_bytes();
                } else {
                    result.stdout = format!("{query}: not found\n").into_bytes();
                }
            }
        }
        _ => return None,
    }

    Some(result)
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

fn write_bytes_output(bytes: &[u8], stream: OutputStream, redirects: &RedirectSpec) {
    let redirection = match stream {
        OutputStream::Stdout => &redirects.stdout,
        OutputStream::Stderr => &redirects.stderr,
    };

    if let Some((path, mode)) = redirection {
        if let Ok(mut file) = open_redirect_file(path, *mode) {
            let _ = file.write_all(bytes);
        }
        return;
    }

    match stream {
        OutputStream::Stdout => {
            let _ = io::stdout().write_all(bytes);
            let _ = io::stdout().flush();
        }
        OutputStream::Stderr => {
            let _ = io::stderr().write_all(bytes);
            let _ = io::stderr().flush();
        }
    }
}

fn write_output(text: &str, stream: OutputStream, redirects: &RedirectSpec) {
    write_bytes_output(text.as_bytes(), stream, redirects);
}

fn run_external_capture(stage: &PipelineStage, input: &[u8]) -> io::Result<CommandResult> {
    let mut command = Command::new(&stage.cmd);
    command.args(&stage.args);
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    command.stdin(Stdio::piped());

    let mut child = command.spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input);
    }

    let output = child.wait_with_output()?;
    Ok(CommandResult {
        stdout: output.stdout,
        stderr: output.stderr,
        should_exit: false,
    })
}

fn build_pipeline_stages(segments: Vec<Vec<ParsedToken>>) -> Vec<PipelineStage> {
    let mut stages = Vec::new();
    for segment in segments {
        let (tokens, redirects) = parse_redirections(segment);
        let Some(cmd) = tokens.first().cloned() else {
            continue;
        };
        stages.push(PipelineStage {
            cmd,
            args: tokens[1..].to_vec(),
            redirects,
        });
    }
    stages
}

fn execute_external_pipeline(stages: &[PipelineStage]) {
    if stages.is_empty() {
        return;
    }

    for stage in stages {
        if find_in_path(&stage.cmd).is_none() {
            write_output(
                &format!("{}: command not found\n", stage.cmd),
                OutputStream::Stdout,
                &stage.redirects,
            );
            return;
        }
        ensure_redirect_files(&stage.redirects);
    }

    let mut children = Vec::new();
    let mut previous_stdout = None;
    let last_index = stages.len() - 1;

    for (idx, stage) in stages.iter().enumerate() {
        let mut command = Command::new(&stage.cmd);
        command.args(&stage.args);

        if let Some(stdout) = previous_stdout.take() {
            command.stdin(Stdio::from(stdout));
        }

        if idx < last_index {
            command.stdout(Stdio::piped());
        } else if let Some((path, mode)) = &stage.redirects.stdout {
            if let Ok(file) = open_redirect_file(path, *mode) {
                command.stdout(Stdio::from(file));
            }
        }

        if let Some((path, mode)) = &stage.redirects.stderr {
            if let Ok(file) = open_redirect_file(path, *mode) {
                command.stderr(Stdio::from(file));
            }
        }

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(_) => {
                write_output(
                    &format!("{}: command not found\n", stage.cmd),
                    OutputStream::Stdout,
                    &stage.redirects,
                );
                return;
            }
        };

        if idx < last_index {
            previous_stdout = child.stdout.take();
        }
        children.push(child);
    }

    for child in &mut children {
        let _ = child.wait();
    }
}

fn execute_mixed_pipeline(stages: &[PipelineStage]) {
    let mut stdin_buffer = Vec::new();

    for (idx, stage) in stages.iter().enumerate() {
        ensure_redirect_files(&stage.redirects);
        let is_last = idx + 1 == stages.len();

        let result = if let Some(result) = run_builtin(&stage.cmd, &stage.args, false, false) {
            result
        } else {
            if find_in_path(&stage.cmd).is_none() {
                write_output(
                    &format!("{}: command not found\n", stage.cmd),
                    OutputStream::Stdout,
                    &stage.redirects,
                );
                return;
            }

            match run_external_capture(stage, &stdin_buffer) {
                Ok(result) => result,
                Err(_) => {
                    write_output(
                        &format!("{}: command not found\n", stage.cmd),
                        OutputStream::Stdout,
                        &stage.redirects,
                    );
                    return;
                }
            }
        };

        if !result.stderr.is_empty() {
            write_bytes_output(&result.stderr, OutputStream::Stderr, &stage.redirects);
        }

        if is_last {
            write_bytes_output(&result.stdout, OutputStream::Stdout, &stage.redirects);
        } else {
            stdin_buffer = result.stdout;
        }
    }
}

fn execute_pipeline(segments: Vec<Vec<ParsedToken>>) {
    let stages = build_pipeline_stages(segments);
    if stages.is_empty() {
        return;
    }

    if stages.iter().all(|stage| !is_builtin_command(&stage.cmd)) {
        execute_external_pipeline(&stages);
    } else {
        execute_mixed_pipeline(&stages);
    }
}

fn main() {
    #[cfg(unix)]
    let _raw_mode = RawModeGuard::new(STDIN_FILENO).ok();

    loop {
        print!("{PROMPT}");
        io::stdout().flush().unwrap();

        let Some(input) = read_user_input().unwrap() else {
            break; // EOF
        };

        let tokens = parse_line(&input);
        let mut pipeline_segments = split_pipeline(tokens);
        if pipeline_segments.len() > 1 {
            execute_pipeline(pipeline_segments);
            continue;
        }

        let segment = pipeline_segments.pop().unwrap_or_default();
        let (tokens, redirects) = parse_redirections(segment);
        let Some(cmd) = tokens.first().cloned() else {
            continue;
        };
        let args = tokens[1..].to_vec();
        ensure_redirect_files(&redirects);

        if let Some(result) = run_builtin(&cmd, &args, true, true) {
            if !result.stdout.is_empty() {
                write_bytes_output(&result.stdout, OutputStream::Stdout, &redirects);
            }
            if !result.stderr.is_empty() {
                write_bytes_output(&result.stderr, OutputStream::Stderr, &redirects);
            }
            if result.should_exit {
                break;
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
