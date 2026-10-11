use std::{
    env,
    io::{self, Write},
    process::ExitCode,
};

use kvkit::Store;

const USAGE: &str = "usage: kvctl --dir DIR <command>

commands:
    set KEY VALUE   set KEY to VALUE
    get KEY         print the value of KEY
    del KEY         delete KEY
    scan-all        print every key and value, sorted by key
    load COUNT      set COUNT keys, key00000000 and up";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("kvctl: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> io::Result<ExitCode> {
    let [flag, dir, command, rest @ ..] = args else {
        return Ok(usage());
    };
    if flag != "--dir" {
        return Ok(usage());
    }
    let mut store = Store::open(dir)?;
    let mut stdout = io::stdout().lock();

    match (command.as_str(), rest) {
        ("set", [key, value]) => store.set(key.as_bytes(), value.as_bytes())?,
        ("get", [key]) => match store.get(key.as_bytes())? {
            Some(value) => {
                stdout.write_all(&value)?;
                stdout.write_all(b"\n")?;
            }
            None => {
                eprintln!("kvctl: {key}: not found");
                return Ok(ExitCode::FAILURE);
            }
        },
        ("del", [key]) => {
            if !store.delete(key.as_bytes())? {
                eprintln!("kvctl: {key}: not found");
                return Ok(ExitCode::FAILURE);
            }
        }
        ("scan-all", []) => {
            for (key, value) in store.scan_all()? {
                stdout.write_all(&key)?;
                stdout.write(b"\t")?;
                stdout.write_all(&value)?;
                stdout.write_all(b"\n")?;
            }
        }
        ("load", [count]) => {
            let Ok(count) = count.parse::<u64>() else {
                return Ok(usage());
            };
            for i in 0..count {
                let key = format!("key{i:08}");
                let value = format!("value{i:08}");
                store.set(key.as_bytes(), value.as_bytes())?;
            }
            println!("set {count} keys");
        }
        _ => return Ok(usage()),
    }

    Ok(ExitCode::SUCCESS)
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}
