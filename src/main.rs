use std::{
    env,
    fs::{self, File},
    io::{self, BufRead, BufReader, stdin},
    iter::Peekable,
    ops::RangeInclusive,
    os::unix::fs::MetadataExt,
    str::CharIndices,
};

fn main() -> Result<()> {
    let args = Args::parse(env::args())?;

    let mut editor = match args.file_path {
        Some(file_path) => Editor::from_file(file_path)?,
        None => Editor::blank(),
    };

    loop {
        let mut input = String::new();
        match stdin().read_line(&mut input) {
            // 'If this function returns `Ok(0)`, the stream has reached EOF.'
            Ok(0) => break,
            Ok(_) => (),
            Err(err) => {
                if args.debug {
                    eprintln!("Failed to read line: {err}");
                }
                println!("?");
                continue;
            }
        };

        // Strip off the newline
        let input = &input[..input.len() - 1];

        if let Err(err) = editor.interpret(input) {
            if args.debug {
                eprintln!("Error: {err}");
            }
            println!("?");
        }
    }

    Ok(())
}

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

#[derive(Debug)]
struct Editor {
    current_address: usize,
    lines: Vec<String>,
    default_filename: Option<String>,
}

impl Editor {
    pub fn from_file(path: String) -> Result<Self> {
        let Ok(file) = File::open(&path) else {
            return Err("Failed to open file");
        };

        let lines = BufReader::new(file)
            .lines()
            .map_while(io::Result::ok)
            .collect::<Vec<String>>();

        if let Ok(metadata) = fs::metadata(&path) {
            println!("{}", metadata.size());
        }

        Ok(Self {
            current_address: lines.len(),
            lines,
            default_filename: Some(path),
        })
    }

    pub fn blank() -> Self {
        Self {
            current_address: 1,
            lines: Vec::new(),
            default_filename: None,
        }
    }

    pub fn interpret(&mut self, input: &str) -> Result<()> {
        let command = Command::parse(input)?;
        self.execute(command)
    }

    fn execute(&mut self, command: Command) -> Result<()> {
        let range = self.resolve_address(&command)?;

        match command.kind {
            CommandToken::Print => {
                println!("{}", self.lines[range.clone()].join("\n"));
                self.current_address = *range.end() + 1;
            }
            CommandToken::PrintAndSet => {
                println!("{}", self.lines[*range.start()]);
                self.current_address = *range.end() + 1;
            }
            CommandToken::Edit(path) => {
                *self = Editor::from_file(path.to_owned())?;
            }
            CommandToken::Write(path) => {
                let Some(path) = path.or(self.default_filename.as_deref()) else {
                    return Err("Missing path to write");
                };

                let mut contents = self.lines[range].join("\n");
                if !contents.ends_with('\n') {
                    contents.push('\n');
                }

                if let Err(_) = fs::write(path, &contents) {
                    return Err("Failed to write file to disk");
                }

                println!("{}", contents.len());

                self.default_filename = Some(path.to_owned());
            }
        }

        Ok(())
    }

    fn resolve_address(&self, command: &Command) -> Result<RangeInclusive<usize>> {
        let (start, end) = match command.address {
            None => match command.kind {
                CommandToken::Print => (self.current_address, self.current_address),
                CommandToken::PrintAndSet => (self.current_address + 1, self.current_address + 1),
                // FIXME: Get rid of this dummy range
                CommandToken::Edit(_) => return Ok(0..=0),
                CommandToken::Write(_) => (1, self.lines.len()),
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
            Ok(start - 1..=end - 1)
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

#[derive(Debug)]
struct Command<'input> {
    address: Option<Address>,
    kind: CommandToken<'input>,
}

impl<'input> Command<'input> {
    pub fn parse(input: &'input str) -> Result<Self> {
        let mut stream = input.char_indices().peekable();

        let address = Address::parse(&mut stream, input)?;
        let kind = CommandToken::parse(&mut stream, input)?;

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
            Some((start, '0'..='9')) => {
                let mut end = start;

                #[allow(clippy::manual_is_ascii_check)]
                while let Some((start, _)) = stream.next_if(|(_, c)| matches!(c, '0'..='9')) {
                    end = start;
                }

                let Ok(number) = command[start..=end].parse() else {
                    return Err("Failed to parse numeric address");
                };

                Ok(Some(AddressToken::Number(number)))
            }
            _ => unreachable!(),
        }
    }
}

#[derive(Debug)]
enum CommandToken<'input> {
    Print,
    PrintAndSet,
    Edit(&'input str),
    Write(Option<&'input str>),
}

impl<'input> CommandToken<'input> {
    pub fn parse(stream: &mut InputStream, input: &'input str) -> Result<Self> {
        let swallow = |stream: &mut InputStream| {
            stream.next().map(|(start, _)| {
                let mut end = start;
                while let Some((start, _)) = stream.next() {
                    end = start;
                }
                &input[start..=end]
            })
        };
        match stream.next().map(|(_, c)| c) {
            None => Ok(CommandToken::PrintAndSet),
            Some('p') => Ok(CommandToken::Print),
            Some('e') => match swallow(stream) {
                Some(path) => Ok(CommandToken::Edit(path.trim_start())),
                None => Err("Missing path for `Edit`"),
            },
            Some('w') => Ok(CommandToken::Write(
                swallow(stream).map(|path| path.trim_start()),
            )),
            _ => Err("Unknown command"),
        }
    }
}

type InputStream<'a> = Peekable<CharIndices<'a>>;

type Result<T> = std::result::Result<T, &'static str>;
