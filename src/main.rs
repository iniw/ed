use std::{env, fs, io::stdin, iter::Peekable, ops::RangeInclusive, str::CharIndices};

#[derive(Default)]
struct Args {
    file_path: Option<String>,
    debug: bool,
}

impl Args {
    fn parse(args: env::Args) -> Result<Self, &'static str> {
        let mut out = Self::default();

        for arg in args.skip(1) {
            match arg.as_str() {
                "-d" | "--debug" => out.debug = true,
                _ => {
                    if out.file_path.is_some() {
                        return Err("Multiple file paths provided");
                    }
                    out.file_path = Some(arg);
                }
            }
        }

        Ok(out)
    }
}

fn main() -> Result<(), &'static str> {
    let args = Args::parse(env::args())?;

    let Some(file_path) = args.file_path else {
        return Err("File path argument must be provided");
    };

    let Ok(file) = fs::read_to_string(&file_path) else {
        return Err("Failed to read file from disk");
    };

    println!("{}", file.len());

    let mut lines = file.lines().collect::<Vec<_>>();

    let mut state = State { cursor: 0 };

    loop {
        let mut cmd = String::new();
        match stdin().read_line(&mut cmd) {
            // 'If this function returns `Ok(0)`, the stream has reached EOF.'
            Ok(0) => break,
            Ok(_) => (),
            Err(err) => {
                if args.debug {
                    eprintln!("Failed to read line: {:?}", err);
                }
                println!("?");
                continue;
            }
        };

        // Strip off the newline
        let cmd = &cmd[..cmd.len() - 1];

        match parse_cmd(cmd).and_then(|cmd| execute_cmd(&mut lines, &mut state, cmd)) {
            Ok(_) => {}
            Err(err) => {
                if args.debug {
                    eprintln!("Error: {:?}", err);
                }
                println!("?");
                continue;
            }
        }
    }

    Ok(())
}

fn execute_cmd(lines: &mut Vec<&str>, state: &mut State, cmd: Cmd) -> Result<(), &'static str> {
    let range_from_addr = |addr: Option<Addr>| -> Result<RangeInclusive<usize>, &'static str> {
        let addr_kind_range = |addr_kind: AddrKind| {
            let n = match addr_kind {
                AddrKind::Dollar => lines.len(),
                AddrKind::Number(addr) => addr,
            };
            if n == 0 || n > lines.len() {
                Err("Out of bounds")
            } else {
                Ok(n - 1)
            }
        };

        match addr {
            None if state.cursor >= lines.len() => Err("Out of bounds"),
            None => Ok(state.cursor..=state.cursor),
            Some(addr) => match addr {
                Addr::Single(addr) => {
                    let range = addr_kind_range(addr)?;
                    Ok(range..=range)
                }
                Addr::Range { begin, end } => Ok(addr_kind_range(begin)?..=addr_kind_range(end)?),
            },
        }
    };

    let range = range_from_addr(cmd.addr)?;
    let end = *range.end();

    match cmd.kind {
        CmdKind::Print => {
            println!("{}", lines[range].join("\n"));
        }
        CmdKind::PrintAndMove => {
            println!("{}", lines[range].join("\n"));
            state.cursor = end + 1;
        }
    }

    Ok(())
}

#[derive(Debug)]
struct State {
    cursor: usize,
}

type Stream<'a> = Peekable<CharIndices<'a>>;

fn parse_cmd(line: &str) -> Result<Cmd, &'static str> {
    let mut stream = line.char_indices().peekable();

    let addr = parse_addr(line, &mut stream)?;
    let kind = parse_cmd_kind(&mut stream);

    Ok(Cmd { addr, kind })
}

fn parse_addr(line: &str, stream: &mut Stream) -> Result<Option<Addr>, &'static str> {
    fn parse_addr_kind(line: &str, stream: &mut Stream) -> Result<Option<AddrKind>, &'static str> {
        match stream.peek().copied() {
            None => Ok(None),
            Some((_, '$')) => {
                // Consume the dollar sign
                stream.next();
                Ok(Some(AddrKind::Dollar))
            }
            Some((begin, '0'..='9')) => {
                let mut end = begin;
                while let Some((off, _)) = stream.next_if(|(_, c)| matches!(c, '0'..='9')) {
                    end = off;
                }

                let Ok(number) = line[begin..=end].parse() else {
                    return Err("Failed to parse numeric address");
                };

                Ok(Some(AddrKind::Number(number)))
            }
            Some(_) => Ok(None),
        }
    }

    let Some(begin) = parse_addr_kind(line, stream)? else {
        return Ok(None);
    };

    let addr = match stream.next_if(|(_, c)| *c == ',') {
        None => Addr::Single(begin),
        Some(_) => match parse_addr_kind(line, stream)? {
            None => Addr::Single(begin),
            Some(end) => Addr::Range { begin, end },
        },
    };

    Ok(Some(addr))
}

fn parse_cmd_kind(stream: &mut Stream) -> CmdKind {
    match stream.next().map(|(_, c)| c) {
        None => CmdKind::PrintAndMove,
        Some('p') => CmdKind::Print,
        other => todo!("No cmd for {:?}", other),
    }
}

#[derive(Debug)]
struct Cmd {
    addr: Option<Addr>,
    kind: CmdKind,
}

#[derive(Debug)]
enum Addr {
    Single(AddrKind),
    Range { begin: AddrKind, end: AddrKind },
}

#[derive(Debug, Clone, Copy)]
enum AddrKind {
    Dollar,
    Number(usize),
}

#[derive(Debug)]
enum CmdKind {
    Print,
    PrintAndMove,
}
