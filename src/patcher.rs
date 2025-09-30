use std::{cmp, io};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Failed to read the input protobuf file: {0}")]
    Read(io::Error),
    #[error("Failed to write the output protobuf file: {0}")]
    Write(io::Error),
    #[error("Failed to parse the protobuf file: Invalid parser state encountered")]
    InvalidState,
}

#[derive(cmp::PartialEq, Debug)]
pub enum Outcome {
    Untouched,
    Replaced,
}

enum State {
    None,
    CommentPending(usize, CommentContext),
    CommentSingleLine(usize, CommentContext),
    CommentMultiLine(usize, CommentContext),
    CommentMultiLineEndPending(usize, CommentContext),
    Edition(usize, usize),
    EditionWhitespacePost(usize),
    EditionEqual(usize),
    EditionEqualWhitespacePost(usize),
    EditionOpenQuote(usize),
    EditionValueEscape(usize),
    EditionValue(usize),
    EditionCloseQuote(usize),
    Complete(Option<(usize, usize)>),
}

impl State {
    fn next_token(self, ch: u8, pos: usize) -> Result<Self, Error> {
        Ok(match self {
            Self::Complete(_) => self,
            Self::None => match ch {
                b'e' => Self::Edition(pos, 0),
                b'/' => Self::CommentPending(pos, self.try_into()?),
                c if c.is_ascii_whitespace() => Self::None,
                _ => Self::Complete(None),
            },
            Self::CommentPending(start_at, ctx) => match ch {
                b'/' => Self::CommentSingleLine(start_at, ctx),
                b'*' => Self::CommentMultiLine(start_at, ctx),
                _ => Self::Complete(None),
            },
            Self::CommentSingleLine(start_at, ctx) => match ch {
                b'\n' => ctx.into(),
                _ => Self::CommentSingleLine(start_at, ctx),
            },
            Self::CommentMultiLine(start_at, ctx) => match ch {
                b'*' => Self::CommentMultiLineEndPending(start_at, ctx),
                _ => Self::CommentMultiLine(start_at, ctx),
            },
            Self::CommentMultiLineEndPending(start_at, ctx) => match ch {
                b'/' => ctx.into(),
                _ => Self::CommentMultiLine(start_at, ctx),
            },
            Self::Edition(start_at, idx) => match (idx, ch) {
                (0, b'd') | (1, b'i') | (2, b't') | (3, b'i') | (4, b'o') | (5, b'n') => {
                    Self::Edition(start_at, idx + 1)
                }
                (6, b'=') => Self::EditionEqual(start_at),
                (6, b'/') => Self::CommentPending(start_at, self.try_into()?),
                (6, c) if c.is_ascii_whitespace() => Self::EditionWhitespacePost(start_at),
                _ => Self::Complete(None),
            },
            Self::EditionWhitespacePost(start_at) => match ch {
                b'=' => Self::EditionEqual(start_at),
                b'/' => Self::CommentPending(start_at, self.try_into()?),
                c if c.is_ascii_whitespace() => Self::EditionWhitespacePost(start_at),
                _ => Self::Complete(None),
            },
            Self::EditionEqual(start_at) | Self::EditionEqualWhitespacePost(start_at) => match ch {
                b'"' => Self::EditionOpenQuote(start_at),
                b'/' => Self::CommentPending(start_at, self.try_into()?),
                c if c.is_ascii_whitespace() => Self::EditionEqualWhitespacePost(start_at),
                _ => Self::Complete(None),
            },
            Self::EditionOpenQuote(start_at) | Self::EditionValue(start_at) => match ch {
                b'"' => Self::EditionCloseQuote(start_at),
                b'\\' => Self::EditionValueEscape(start_at),
                _ => Self::EditionValue(start_at),
            },
            Self::EditionValueEscape(start_at) => Self::EditionValue(start_at),
            Self::EditionCloseQuote(start_at) => Self::Complete(Some((start_at, pos))),
        })
    }

    fn get_bounds(&self) -> Option<(usize, Option<usize>)> {
        match self {
            &Self::Complete(None) | Self::None => None,
            &Self::Complete(Some((to, from))) => Some((to, Some(from))),
            &Self::CommentPending(to, _)
            | &Self::CommentSingleLine(to, _)
            | &Self::CommentMultiLine(to, _)
            | &Self::CommentMultiLineEndPending(to, _)
            | &Self::Edition(to, _)
            | &Self::EditionWhitespacePost(to)
            | &Self::EditionEqual(to)
            | &Self::EditionEqualWhitespacePost(to)
            | &Self::EditionOpenQuote(to)
            | &Self::EditionValueEscape(to)
            | &Self::EditionValue(to)
            | &Self::EditionCloseQuote(to) => Some((to, None)),
        }
    }
}

enum CommentContext {
    None,
    EditionWhitespacePost(usize),
    EditionEqual(usize),
    EditionEqualWhitespacePost(usize),
}

impl From<CommentContext> for State {
    fn from(value: CommentContext) -> Self {
        match value {
            CommentContext::None => Self::None,
            CommentContext::EditionWhitespacePost(pos) => Self::EditionWhitespacePost(pos),
            CommentContext::EditionEqual(pos) => Self::EditionEqual(pos),
            CommentContext::EditionEqualWhitespacePost(pos) => {
                Self::EditionEqualWhitespacePost(pos)
            }
        }
    }
}

impl TryFrom<State> for CommentContext {
    type Error = Error;

    fn try_from(value: State) -> Result<Self, Self::Error> {
        match value {
            State::None => Ok(Self::None),
            State::EditionWhitespacePost(pos) => Ok(Self::EditionWhitespacePost(pos)),
            State::EditionEqual(pos) => Ok(Self::EditionEqual(pos)),
            State::EditionEqualWhitespacePost(pos) => Ok(Self::EditionEqualWhitespacePost(pos)),
            State::Edition(pos, 6) => Ok(Self::EditionWhitespacePost(pos)),
            _ => Err(Error::InvalidState),
        }
    }
}

pub fn patch_edition(mut src: impl io::BufRead, mut dst: impl io::Write) -> Result<Outcome, Error> {
    let mut line = Vec::with_capacity(1 << 14);
    // let mut line = Vec::with_capacity(30|29);
    let mut state = State::None;
    let mut outcome = Outcome::Untouched;

    while src.read_until(b'\n', &mut line).map_err(Error::Read)? > 0 {
        state = line
            .iter()
            .enumerate()
            .try_fold(state, |state, (pos, &ch)| state.next_token(ch, pos))?;

        match state.get_bounds() {
            Some((to, Some(from))) => {
                dst.write_all(&line[0..to]).map_err(Error::Write)?;
                dst.write_all(r#"syntax = "proto3""#.as_bytes())
                    .map_err(Error::Write)?;
                dst.write_all(&line[from..]).map_err(Error::Write)?;

                line.clear();

                outcome = Outcome::Replaced;
            }
            Some((to, None)) => {
                dst.write_all(&line[0..to]).map_err(Error::Write)?;

                line.drain(0..to);
            }
            None => {
                dst.write_all(&line).map_err(Error::Write)?;

                line.clear();
            }
        }

        state = match state {
            State::Complete(Some(_)) => State::Complete(None),
            State::Complete(None) => state,
            _ => State::None,
        };
    }

    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use std::io;

    #[test]
    fn copy_unchanged() {
        let input = r#"syntax = "proto3";

package crabs;

message Ferris {
  string type = 1;
}
"#;

        let mut output = Vec::new();
        let result = super::patch_edition(io::BufReader::new(input.as_bytes()), &mut output);
        let outcome = result.expect("Faled to copy the data");

        assert_eq!(
            super::Outcome::Untouched,
            outcome,
            "Expected the file to be copied without changes",
        );

        let output = String::from_utf8(output).expect("The output string is corrupted");

        assert_eq!(input, output, "");
    }

    #[test]
    fn copy_replace() {
        let input = r#"edition = "2023";

package crabs;

message Ferris {
  string type = 1;
}
"#;

        let mut output = Vec::new();

        let result = super::patch_edition(io::BufReader::new(input.as_bytes()), &mut output);

        let outcome = result.expect("Faled to copy the data");

        assert_eq!(
            super::Outcome::Replaced,
            outcome,
            "Expected the edition to be replaced with syntax"
        );

        let output = String::from_utf8(output).expect("The resulting copy is corrupted");

        assert_eq!(
            r#"syntax = "proto3";

package crabs;

message Ferris {
  string type = 1;
}
"#,
            output,
        );
    }

    #[test]
    fn copy_unchanged_ignoring_comments() {
        let input = r#"// This is a comment above the edition
syntax = "proto2";

package crabs;

message Ferris {
  string type = 1;
}
"#;

        let mut output = Vec::new();
        let result = super::patch_edition(io::BufReader::new(input.as_bytes()), &mut output);
        let outcome = result.expect("Faled to copy the data");

        assert_eq!(
            super::Outcome::Untouched,
            outcome,
            "Expected the file to be copied without changes",
        );

        let output = String::from_utf8(output).expect("The resulting copy is corrupted");

        assert_eq!(
            r#"// This is a comment above the edition
syntax = "proto2";

package crabs;

message Ferris {
  string type = 1;
}
"#,
            output,
        );
    }

    #[test]
    fn copy_replace_ignoring_comments() {
        let input = r#"/* This is a comment above the edition
and it is a multi-line one */
edition = "2023";

package crabs;

message Ferris {
  string type = 1;
}
"#;

        let mut output = Vec::new();
        let result = super::patch_edition(io::BufReader::new(input.as_bytes()), &mut output);
        let outcome = result.expect("Faled to copy the data");

        assert_eq!(
            super::Outcome::Replaced,
            outcome,
            "Expected the edition to be replaced with syntax"
        );

        let output = String::from_utf8(output).expect("The resulting copy is corrupted");

        assert_eq!(
            r#"/* This is a comment above the edition
and it is a multi-line one */
syntax = "proto3";

package crabs;

message Ferris {
  string type = 1;
}
"#,
            output,
        );
    }

    #[test]
    fn copy_unchanged_ignoring_whitespace() {
        let input = r#"
  syntax = "proto3";

package crabs;

message Ferris {
  string type = 1;
}
"#;

        let mut output = Vec::new();
        let result = super::patch_edition(io::BufReader::new(input.as_bytes()), &mut output);
        let outcome = result.expect("Faled to copy the data");

        assert_eq!(
            super::Outcome::Untouched,
            outcome,
            "Expected the file to be copied without changes",
        );

        let output = String::from_utf8(output).expect("The resulting copy is corrupted");

        assert_eq!(
            r#"
  syntax = "proto3";

package crabs;

message Ferris {
  string type = 1;
}
"#,
            output,
        );
    }

    #[test]
    fn copy_replace_ignoring_whitespace() {
        let input = r#"
  edition = "2023";

package crabs;

message Ferris {
  string type = 1;
}
"#;

        let mut output = Vec::new();
        let result = super::patch_edition(io::BufReader::new(input.as_bytes()), &mut output);
        let outcome = result.expect("Faled to copy the data");

        assert_eq!(
            super::Outcome::Replaced,
            outcome,
            "Expected the edition to be replaced with syntax"
        );

        let output = String::from_utf8(output).expect("The resulting copy is corrupted");

        assert_eq!(
            r#"
  syntax = "proto3";

package crabs;

message Ferris {
  string type = 1;
}
"#,
            output,
        );
    }

    #[test]
    fn copy_unchanged_ignoring_edition_inside_message() {
        let input = r#"syntax = "proto3";

package crabs;

message Ferris {
  string edition = 1;
}
"#;

        let mut output = Vec::new();
        let result = super::patch_edition(io::BufReader::new(input.as_bytes()), &mut output);
        let outcome = result.expect("Faled to copy the data");

        assert_eq!(
            super::Outcome::Untouched,
            outcome,
            "Expected the file to be copied without changes",
        );

        let output = String::from_utf8(output).expect("The resulting copy is corrupted");

        assert_eq!(
            r#"syntax = "proto3";

package crabs;

message Ferris {
  string edition = 1;
}
"#,
            output,
        );
    }

    #[test]
    fn copy_replace_ignore_same_line_comment() {
        let input = r#"/* This is a weird case of the comment */ edition = "2023";

package crabs;

message Ferris {
  string type = 1;
}
"#;

        let mut output = Vec::new();
        let result = super::patch_edition(io::BufReader::new(input.as_bytes()), &mut output);
        let outcome = result.expect("Faled to copy the data");

        assert_eq!(
            super::Outcome::Replaced,
            outcome,
            "Expected the edition to be replaced with syntax"
        );

        let output = String::from_utf8(output).expect("The resulting copy is corrupted");

        assert_eq!(
            r#"/* This is a weird case of the comment */ syntax = "proto3";

package crabs;

message Ferris {
  string type = 1;
}
"#,
            output,
        );
    }

    #[test]
    fn copy_unchanged_ignoring_same_line_comment() {
        let input = r#"/* This is a weird case of the comment */ syntax = "proto3";

package crabs;

message Ferris {
  string type = 1;
}
"#;

        let mut output = Vec::new();
        let result = super::patch_edition(io::BufReader::new(input.as_bytes()), &mut output);
        let outcome = result.expect("Faled to copy the data");

        assert_eq!(
            super::Outcome::Untouched,
            outcome,
            "Expected the file to be copied without changes",
        );

        let output = String::from_utf8(output).expect("The resulting copy is corrupted");

        assert_eq!(
            r#"/* This is a weird case of the comment */ syntax = "proto3";

package crabs;

message Ferris {
  string type = 1;
}
"#,
            output,
        );
    }

    #[test]
    fn copy_unchanged_ignoring_edition_inside_comment() {
        let input = r#"/* We can't yet upgrade to the
edition = "2023";
because not every language compiler supports it.*/
syntax = "proto3";

package crabs;

message Ferris {
  string type = 1;
}
"#;

        let mut output = Vec::new();
        let result = super::patch_edition(io::BufReader::new(input.as_bytes()), &mut output);
        let outcome = result.expect("Faled to copy the data");

        assert_eq!(
            super::Outcome::Untouched,
            outcome,
            "Expected the file to be copied without changes",
        );

        let output = String::from_utf8(output).expect("The resulting copy is corrupted");

        assert_eq!(
            r#"/* We can't yet upgrade to the
edition = "2023";
because not every language compiler supports it.*/
syntax = "proto3";

package crabs;

message Ferris {
  string type = 1;
}
"#,
            output,
        );
    }

    #[test]
    fn copy_replace_ignoring_edition_inside_comment() {
        let input = r#"/* We recently upgraded to the
edition = "2023";
* but we have tooling that replaces it back to
syntax = "proto3";
on as needed basis for languages that don't have proper support */
edition = "2023";

package crabs;

message Ferris {
  string type = 1;
}
"#;

        let mut output = Vec::new();
        let result = super::patch_edition(io::BufReader::new(input.as_bytes()), &mut output);
        let outcome = result.expect("Faled to copy the data");

        assert_eq!(
            super::Outcome::Replaced,
            outcome,
            "Expected the edition to be replaced with syntax",
        );

        let output = String::from_utf8(output).expect("The resulting copy is corrupted");

        assert_eq!(
            r#"/* We recently upgraded to the
edition = "2023";
* but we have tooling that replaces it back to
syntax = "proto3";
on as needed basis for languages that don't have proper support */
syntax = "proto3";

package crabs;

message Ferris {
  string type = 1;
}
"#,
            output,
        );
    }

    #[test]
    fn copy_replace_no_whitespace_in_edition() {
        let input = r#"edition="2023";

package crabs;

message Ferris {}
"#;

        let mut output = Vec::new();
        let result = super::patch_edition(io::BufReader::new(input.as_bytes()), &mut output);
        let outcome = result.expect("Faled to copy the data");

        assert_eq!(
            super::Outcome::Replaced,
            outcome,
            "Expected the edition to be replaced with syntax",
        );

        let output = String::from_utf8(output).expect("The resulting copy is corrupted");

        assert_eq!(
            r#"syntax = "proto3";

package crabs;

message Ferris {}
"#,
            output,
        );
    }

    #[test]
    fn copy_replace_multiple_whitespace_in_edition() {
        let input = r#"edition =		"2023" ;

package crabs;

message Ferris {}
"#;

        let mut output = Vec::new();
        let result = super::patch_edition(io::BufReader::new(input.as_bytes()), &mut output);
        let outcome = result.expect("Faled to copy the data");

        assert_eq!(
            super::Outcome::Replaced,
            outcome,
            "Expected the edition to be replaced with syntax",
        );

        let output = String::from_utf8(output).expect("The resulting copy is corrupted");

        assert_eq!(
            r#"syntax = "proto3" ;

package crabs;

message Ferris {}
"#,
            output,
        );
    }

    #[test]
    fn copy_replace_with_comments_in_edition() {
        let input = r#"edition/* Edition comment */// Weird comment here
= /*This may be 2024 at some point*/"2023"
// Needs to be replaced with syntax for now tho.
;

package crabs;

message Ferris {}
"#;

        let mut output = Vec::new();
        let result = super::patch_edition(io::BufReader::new(input.as_bytes()), &mut output);
        let outcome = result.expect("Faled to copy the data");

        assert_eq!(
            super::Outcome::Replaced,
            outcome,
            "Expected the edition to be replaced with syntax",
        );

        let output = String::from_utf8(output).expect("The resulting copy is corrupted");

        assert_eq!(
            r#"syntax = "proto3"
// Needs to be replaced with syntax for now tho.
;

package crabs;

message Ferris {}
"#,
            output,
        );
    }
}
