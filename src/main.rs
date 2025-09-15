use std::{
    env,
    fs::File,
    io::{self, BufRead, BufReader, stdin},
    iter::Peekable,
    ops::RangeInclusive,
    os::unix::fs::MetadataExt,
    str::CharIndices,
};

#[derive(Default)]
struct Args {
    file_path: Option<String>,
    debug: bool,
}

impl Args {
    fn parse(args: env::Args) -> Result<Self> {
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

fn main() -> Result<()> {
    let args = Args::parse(env::args())?;

    let Some(file_path) = args.file_path else {
        return Err("File path argument must be provided");
    };

    let Ok(file) = File::open(&file_path) else {
        return Err("Failed to open file from disk");
    };

    if let Ok(metadata) = file.metadata() {
        println!("{}", metadata.size());
    }

    let mut editor = Editor::from_file(file);

    loop {
        let mut input = String::new();
        match stdin().read_line(&mut input) {
            // 'If this function returns `Ok(0)`, the stream has reached EOF.'
            Ok(0) => break,
            Ok(_) => (),
            Err(err) => {
                if args.debug {
                    eprintln!("Failed to read line: {err:?}");
                }
                println!("?");
                continue;
            }
        };

        // Strip off the newline
        let input = &input[..input.len() - 1];

        if let Err(err) = editor.interpret(input) {
            if args.debug {
                eprintln!("Error: {err:?}");
            }
            println!("?");
        }
    }

    Ok(())
}

#[derive(Debug)]
struct Editor {
    current_address: usize,
    lines: Lines,
}

impl Editor {
    pub fn from_file(file: File) -> Self {
        let lines = BufReader::new(file)
            .lines()
            .map_while(io::Result::<String>::ok)
            .collect::<Lines>();

        Self {
            current_address: lines.len(),
            lines,
        }
    }

    pub fn interpret(&mut self, input: &str) -> Result<()> {
        let command = Command::parse(input)?;
        self.execute(command)
    }

    fn execute(&mut self, command: Command) -> Result<()> {
        let range = self.resolve_address(&command)?;
        let end = *range.end();
        let addressed_lines = &mut self.lines[range];

        match command.kind {
            CommandToken::Print => {
                println!("{}", addressed_lines.join("\n"));
            }
            CommandToken::PrintAndSet => {
                println!("{}", addressed_lines.join("\n"));
            }
        }

        self.current_address = end;

        Ok(())
    }

    fn resolve_address(&self, command: &Command) -> Result<RangeInclusive<usize>> {
        let (start, end) = match command.address {
            None => match command.kind {
                CommandToken::Print => (self.current_address, self.current_address),
                CommandToken::PrintAndSet => (self.current_address + 1, self.current_address + 1),
            },
            Some(address) => match address {
                Address::Single(single) => {
                    let single = self.resolve_address_token(single);
                    (single, single)
                }
                Address::Range { start, end } => {
                    let start = self.resolve_address_token(start);
                    let end = self.resolve_address_token(end);
                    (start, end)
                }
            },
        };

        let validate = |address: usize| address != 0 && address <= self.lines.len();

        if validate(start) && validate(end) {
            Ok(start..=end)
        } else {
            Err("Out of bounds")
        }
    }

    fn resolve_address_token(&self, address_token: AddressToken) -> usize {
        match address_token {
            AddressToken::Dollar => self.lines.len(),
            AddressToken::Number(addr) => addr,
        }
    }
}

type InputStream<'a> = Peekable<CharIndices<'a>>;

#[derive(Debug)]
struct Command {
    address: Option<Address>,
    kind: CommandToken,
}

impl Command {
    pub fn parse(input: &str) -> Result<Command> {
        let mut stream = input.char_indices().peekable();

        let address = Address::parse(&mut stream, input)?;
        let kind = CommandToken::parse(&mut stream)?;

        match stream.next() {
            None => Ok(Command { address, kind }),
            Some(_) => Err("Extra characters in stream"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Address {
    Single(AddressToken),
    Range {
        start: AddressToken,
        end: AddressToken,
    },
}

impl Address {
    pub fn parse(stream: &mut InputStream, command: &str) -> Result<Option<Self>> {
        let Some(start) = AddressToken::parse(stream, command)? else {
            return Ok(None);
        };

        let address = match stream.next_if(|(_, c)| *c == ',') {
            None => Address::Single(start),
            Some(_) => match AddressToken::parse(stream, command)? {
                None => Address::Single(start),
                Some(end) => Address::Range { start, end },
            },
        };

        Ok(Some(address))
    }
}

#[derive(Debug, Clone, Copy)]
enum AddressToken {
    Dollar,
    Number(usize),
}

impl AddressToken {
    pub fn parse(stream: &mut InputStream, command: &str) -> Result<Option<Self>> {
        match stream.next_if(|(_, c)| matches!(c, '$' | '0'..='9')) {
            None => Ok(None),
            Some((_, '$')) => Ok(Some(AddressToken::Dollar)),
            Some((begin, '0'..='9')) => {
                let mut end = begin;
                #[allow(clippy::manual_is_ascii_check)]
                while let Some((offset, _)) = stream.next_if(|(_, c)| matches!(c, '0'..='9')) {
                    end = offset;
                }

                let Ok(number) = command[begin..=end].parse() else {
                    return Err("Failed to parse numeric address");
                };

                Ok(Some(AddressToken::Number(number)))
            }
            _ => unreachable!(),
        }
    }
}

#[derive(Debug)]
enum CommandToken {
    Print,
    PrintAndSet,
}

impl CommandToken {
    pub fn parse(stream: &mut InputStream) -> Result<CommandToken> {
        match stream.next().map(|(_, c)| c) {
            None => Ok(CommandToken::PrintAndSet),
            Some('p') => Ok(CommandToken::Print),
            _ => Err("Unknown command"),
        }
    }
}

type Lines = Vec<String>;

type Result<T> = std::result::Result<T, &'static str>;
