//! Command line: `bracket [options] [file[:line[:column]]]...`.

use std::ffi::OsString;

use bracket_session::OpenRequest;

#[derive(Debug, PartialEq, Eq)]
pub enum Cli {
    Open(Vec<OpenRequest>),
    Help,
    Version,
    /// An option Bracket does not know.
    Unknown(String),
}

pub fn parse(args: impl IntoIterator<Item = OsString>) -> Cli {
    let mut files = Vec::new();
    let mut options_ended = false;
    for arg in args {
        if !options_ended && let Some(text) = arg.to_str() {
            match text {
                "--" => {
                    options_ended = true;
                    continue;
                }
                "-h" | "--help" | "/?" => return Cli::Help,
                "-v" | "--version" => return Cli::Version,
                option if option.starts_with('-') && option.len() > 1 => {
                    return Cli::Unknown(option.to_owned());
                }
                _ => {}
            }
        }
        files.push(OpenRequest::parse(&arg));
    }
    Cli::Open(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli(args: &[&str]) -> Cli {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn files_and_positions() {
        assert_eq!(cli(&[]), Cli::Open(Vec::new()));
        let Cli::Open(files) = cli(&["a.cs", r"C:\b.json:4:2"]) else {
            panic!("expected files");
        };
        assert_eq!(files[0], OpenRequest::file("a.cs"));
        assert_eq!((files[1].line, files[1].column), (Some(4), Some(2)));
    }

    #[test]
    fn options() {
        assert_eq!(cli(&["--help"]), Cli::Help);
        assert_eq!(cli(&["/?"]), Cli::Help);
        assert_eq!(cli(&["a.cs", "-v"]), Cli::Version);
        assert_eq!(cli(&["--frobnicate"]), Cli::Unknown("--frobnicate".into()));
    }

    #[test]
    fn a_double_dash_ends_the_options() {
        assert_eq!(
            cli(&["--", "-strange-name.txt"]),
            Cli::Open(vec![OpenRequest::file("-strange-name.txt")])
        );
        assert_eq!(cli(&["-"]), Cli::Open(vec![OpenRequest::file("-")]));
    }
}
