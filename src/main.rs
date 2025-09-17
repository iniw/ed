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
        input.pop();

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
enum Mode {
    Command,
    Insert,
}

#[derive(Debug)]
struct Editor {
    current_address: usize,
    lines: Vec<String>,
    default_filename: Option<String>,
    mode: Mode,
}

impl Editor {
    pub fn from_file(path: String) -> Result<Self> {
        let Ok(file) = File::open(&path) else {
            return Err("Failed to open file");
        };

        if let Ok(metadata) = file.metadata() {
            println!("{}", metadata.size());
        }

        let lines = BufReader::new(file)
            .lines()
            .map_while(io::Result::ok)
            .collect::<Vec<String>>();

        Ok(Self {
            current_address: lines.len(),
            lines,
            default_filename: Some(path),
            mode: Mode::Command,
        })
    }

    pub fn blank() -> Self {
        Self {
            current_address: 1,
            lines: Vec::new(),
            default_filename: None,
            mode: Mode::Command,
        }
    }

    pub fn interpret(&mut self, input: String) -> Result<()> {
        match self.mode {
            Mode::Command => {
                let command = Command::parse(&input)?;
                self.execute(command)
            }
            Mode::Insert => {
                if input == "." {
                    self.mode = Mode::Command;
                } else {
                    self.lines.insert(self.current_address, input);
                    self.current_address += 1;
                }
                Ok(())
            }
        }
    }

    fn execute(&mut self, command: Command) -> Result<()> {
        let range = self.resolve_address(&command)?;

        use CommandToken::*;
        match command.kind {
            PrintAndSet => {
                self.current_address = *range.end() + 1;
                println!("{}", self.lines[*range.start()]);
            }
            Print => {
                self.current_address = *range.end() + 1;
                println!("{}", self.lines[range].join("\n"));
            }

            Edit(path) => {
                let Some(path) = path.or(self.default_filename.as_deref()) else {
                    return Err("Missing path to write");
                };

                *self = Editor::from_file(path.to_owned())?;
            }
            Write(path) => {
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

            Append => {
                self.current_address = *range.start() + 1;
                self.mode = Mode::Insert;
            }
            Insert => {
                self.current_address = *range.start();
                self.mode = Mode::Insert;
            }
            Change => {
                self.current_address = *range.start();
                self.lines.drain(range);
                self.mode = Mode::Insert;
            }

            Delete => {
                self.current_address = *range.end() + 1;
                self.lines.drain(range);
            }
        }

        Ok(())
    }

    fn resolve_address(&self, command: &Command) -> Result<RangeInclusive<usize>> {
        use CommandToken::*;
        let current_address = (self.current_address, self.current_address);
        let (start, end) = match command.address {
            None => match command.kind {
                PrintAndSet => (self.current_address + 1, self.current_address + 1),
                Print => current_address,

                // FIXME: Get rid of this dummy range
                Edit(_) => return Ok(usize::MAX..=usize::MAX),
                Write(_) => (1, self.lines.len()),

                Append => current_address,
                Insert => current_address,
                Change => current_address,

                Delete => current_address,
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
        let mut stream = InputStream::from_input(input);

        let address = Address::parse(&mut stream)?;
        let kind = CommandToken::parse(&mut stream)?;

        match stream.next() {
            None => Ok(Command { address, kind }),
            Some(_) => unreachable!("Entire stream should be consumed on successful parsed"),
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
    pub fn parse(stream: &mut InputStream) -> Result<Option<Self>> {
        let Some(start) = AddressToken::parse(stream)? else {
            return Ok(None);
        };

        let address = match stream.next_if_eq(',') {
            None => Address::Single(start),
            Some(_) => match AddressToken::parse(stream)? {
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
    pub fn parse(stream: &mut InputStream) -> Result<Option<Self>> {
        match stream.next_if_with_index(|c| matches!(c, '$' | '0'..='9')) {
            None => Ok(None),
            Some((_, '$')) => Ok(Some(AddressToken::Dollar)),
            Some((start, '0'..='9')) => {
                let str = stream.consume_while(start, |c| matches!(c, '0'..='9'));
                let Ok(number) = str.parse() else {
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
    PrintAndSet,
    Print,

    Edit(Option<&'input str>),
    Write(Option<&'input str>),

    Append,
    Insert,
    Change,

    Delete,
}

impl<'input> CommandToken<'input> {
    pub fn parse(stream: &mut InputStream<'input>) -> Result<Self> {
        let swallow = |stream: &mut InputStream<'input>| match stream.next() {
            Some(c) if c.is_whitespace() => {
                // Consume leading whitespace
                while stream.next_if(char::is_whitespace).is_some() {}

                match stream.next_with_index() {
                    Some((start, _)) => Ok(Some(stream.consume(start))),
                    None => Err("Missing argument for command"),
                }
            }
            Some(_) => Err("Expected space after command"),
            None => Ok(None),
        };

        use CommandToken::*;
        let command = match stream.next() {
            None => PrintAndSet,
            Some('p') => Print,

            Some('e') => Edit(swallow(stream)?),
            Some('w') => Write(swallow(stream)?),

            Some('a') => Append,
            Some('i') => Insert,
            Some('c') => Change,

            Some('d') => Delete,

            _ => return Err("Unknown command"),
        };

        Ok(command)
    }
}

struct InputStream<'input> {
    input: &'input str,
    stream: Peekable<CharIndices<'input>>,
}

impl<'input> InputStream<'input> {
    fn from_input(input: &'input str) -> Self {
        Self {
            input,
            stream: input.char_indices().peekable(),
        }
    }

    fn next(&mut self) -> Option<char> {
        self.stream.next().map(|(_, c)| c)
    }

    fn next_if(&mut self, f: impl FnOnce(char) -> bool) -> Option<char> {
        self.stream.next_if(|(_, c)| f(*c)).map(|(_, c)| c)
    }

    fn next_if_eq(&mut self, c: char) -> Option<char> {
        self.next_if(|nc| nc == c)
    }

    fn next_with_index(&mut self) -> Option<(usize, char)> {
        self.stream.next()
    }

    fn next_if_with_index(&mut self, f: impl FnOnce(char) -> bool) -> Option<(usize, char)> {
        self.stream.next_if(|(_, c)| f(*c))
    }

    fn consume(&mut self, start: usize) -> &'input str {
        let mut end = start;
        while let Some((start, _)) = self.stream.next() {
            end = start;
        }
        &self.input[start..=end]
    }

    fn consume_while(&mut self, start: usize, f: impl Fn(char) -> bool) -> &'input str {
        let mut end = start;
        while let Some((start, _)) = self.next_if_with_index(&f) {
            end = start;
        }
        &self.input[start..=end]
    }
}

type Result<T> = std::result::Result<T, &'static str>;
