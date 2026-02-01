use std::io::{self, Write};

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
                    _ => {
                        println!("{query}: not found");
                    }
                }
            }
            continue;
        }

        println!("{cmd}: command not found");
    }
}
