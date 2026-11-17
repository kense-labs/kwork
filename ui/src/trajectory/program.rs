//! Program eligibility and summaries mirror upstream code-program.ts. The
//! actual source viewer resolves language from the recorded schema upstream.
use super::{Kind, Record};
use serde_json::Value;

pub(crate) struct Program {
    pub description: String,
}

pub(crate) fn is_program_tool(name: &str) -> bool {
    matches!(name, "run_code" | "cua_run_code")
}

fn parse(kind: Kind, name: &str, input: &str) -> Option<Program> {
    if !matches!(kind, Kind::Tool | Kind::Subtool) || !is_program_tool(name) {
        return None;
    }
    let args: Value = serde_json::from_str(input).ok()?;
    let code = args.get("code")?.as_str()?;
    let description = match args.get("description") {
        Some(value) => value.as_str()?,
        None => "",
    };
    let description = if description.trim().is_empty() {
        code.lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .trim()
    } else {
        description
    };
    Some(Program {
        description: description.to_owned(),
    })
}

impl Record {
    pub fn program(&self) -> Option<Program> {
        parse(self.kind, &self.name, &self.entry.input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn program_uses_recorded_arguments_and_preserves_explicit_description() {
        let input = json!({"code":"\n  print('ok')\n", "description":"  Meaningful description\n"})
            .to_string();
        assert_eq!(
            parse(Kind::Tool, "run_code", &input).unwrap().description,
            "  Meaningful description\n"
        );
        let input = json!({"code":"\r\n  print('ok')\r\n", "description":"  "}).to_string();
        assert_eq!(
            parse(Kind::Subtool, "cua_run_code", &input)
                .unwrap()
                .description,
            "print('ok')"
        );
    }

    #[test]
    fn malformed_or_unrelated_calls_keep_the_generic_inspector() {
        for input in [
            "null",
            "[]",
            "{",
            r#"{"code":false}"#,
            r#"{"code":"ok","description":null}"#,
        ] {
            assert!(parse(Kind::Tool, "run_code", input).is_none());
        }
        let input = r#"{"code":"ok"}"#;
        assert!(parse(Kind::Tool, "bash", input).is_none());
        assert!(parse(Kind::Message, "run_code", input).is_none());
        assert!(parse(Kind::Tool, "run_code", input).is_some());
    }
}
