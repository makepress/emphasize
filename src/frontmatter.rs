use eyre::Report;
use serde::{Deserialize, Serialize};

use crate::Result;

enum State {
    SearchForStart,
    ReadingMarker { count: usize, end: bool },
    ReadingFrontMatter { buf: String, line_start: bool },
    SkipNewLine { end: bool },
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("EOF while parsing frontmatter")]
    Eof,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FrontMatter {
    pub title: String,
    pub date: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub template: Option<String>,
}

impl FrontMatter {
    pub fn parse(name: &str, input: &str) -> Result<(Self, usize)> {
        let mut state = State::SearchForStart;

        let mut payload = None;
        let offset;

        let mut chars = input.char_indices();
        'parse: loop {
            let (idx, ch) = match chars.next() {
                Some(x) => x,
                None => return Err(Error::Eof.into()),
            };
            match &mut state {
                State::SearchForStart => match ch {
                    '-' => {
                        state = State::ReadingMarker {
                            count: 1,
                            end: false,
                        };
                    }
                    '\n' | '\t' | ' ' => {}
                    _ => {
                        panic!("Start of frontmatter not found");
                    }
                },
                State::ReadingMarker { count, end } => match ch {
                    '-' => {
                        *count += 1;
                        if *count == 3 {
                            state = State::SkipNewLine { end: *end };
                        }
                    }
                    _ => {
                        panic!("Malformed frontmatter marker");
                    }
                },
                State::SkipNewLine { end } => match ch {
                    '\n' => {
                        if *end {
                            offset = idx + 1;
                            break 'parse;
                        } else {
                            state = State::ReadingFrontMatter {
                                buf: String::new(),
                                line_start: true,
                            }
                        }
                    }
                    _ => panic!("Expected newline, got {:?}", ch),
                },
                State::ReadingFrontMatter { buf, line_start } => match ch {
                    '-' if *line_start => {
                        let mut state_temp = State::ReadingMarker {
                            count: 1,
                            end: true,
                        };
                        std::mem::swap(&mut state, &mut state_temp);
                        if let State::ReadingFrontMatter { buf, .. } = state_temp {
                            payload = Some(buf);
                        } else {
                            unreachable!();
                        }
                    }
                    ch => {
                        buf.push(ch);
                        *line_start = ch == '\n';
                    }
                },
            }
        }

        let payload = payload.unwrap();

        let fm: Self = serde_yaml::from_str(&payload)
            .map_err(|e| Report::from(e).wrap_err(format!("while parsing {:?}", name)))?;

        Ok((fm, offset))
    }
}
