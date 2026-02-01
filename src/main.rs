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

        if let Some(cmd) = input.split_whitespace().next() {
            println!("{cmd}: command not found");
        }
    }
}
